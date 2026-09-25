/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Build augmented manifests for trees at upload time, before the changeset
//! that references them exists.

use std::collections::HashMap;
use std::sync::Arc;

use acl_manifest::AclChildNode;
use acl_manifest::DirectoryAclInputs;
use acl_manifest::acl_node_for_directory;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Entry;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestEnvelope;
use mercurial_types::HgNodeHash;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::sharded_augmented_manifest::HgAugmentedDirectoryNode;
use mononoke_types::MPathElement;
use mononoke_types::acl_manifest::AclManifestDirectoryEntry;
use restricted_paths_common::RestrictedPathsConfigBased;

use crate::derive_hg_augmented_manifest::derive_augmented_manifest_for_uploaded_tree;

const MAX_CONCURRENT_CHILD_LOOKUPS: usize = 100;

#[derive(Debug)]
pub struct BuiltTree {
    pub directory: HgAugmentedDirectoryNode,
    /// Carries the node's flags as well as its id, so the containing directory
    /// can list this tree as a child without loading it back.
    pub acl: Option<AclManifestDirectoryEntry>,
}

struct ChildNode {
    directory: HgAugmentedDirectoryNode,
    acl: Option<AclChildNode>,
}

struct ParsedTree {
    node_id: HgNodeHash,
    manifest: HgBlobManifest,
}

fn child_directories(
    manifest: &HgBlobManifest,
) -> impl Iterator<Item = (&MPathElement, HgNodeHash)> {
    manifest
        .content()
        .files
        .iter()
        .filter_map(|(name, entry)| match entry {
            Entry::Tree(id) => Some((name, id.into_nodehash())),
            Entry::Leaf(_) => None,
        })
}

/// One tree's child directories, split by where each has to be resolved from.
#[derive(Default)]
pub struct TreeChildren {
    /// Children that arrived in the same batch.
    pub in_batch: Vec<(MPathElement, HgNodeHash)>,
    /// Children that did not, so they must already be derived.
    pub external: Vec<(MPathElement, HgNodeHash)>,
}

/// A batch of uploaded trees, parsed and ordered by containment. Built from the
/// uploaded bytes alone, so its shape is known before the blobstore is touched.
pub struct UploadedTreeBatch {
    trees: Vec<ParsedTree>,
    /// Per tree, the positions of the batch trees it directly contains. Names
    /// are irrelevant to ordering, hence separate from `children`.
    contains: Vec<Vec<usize>>,
    /// Per tree, its children split by where they resolve from.
    children: Vec<TreeChildren>,
}

/// One tree of a batch, reached in build order.
pub struct OrderedTree<'a> {
    pub node_id: HgNodeHash,
    pub manifest: &'a HgBlobManifest,
    pub children: &'a TreeChildren,
}

impl UploadedTreeBatch {
    /// Edges are hg node ids, not paths: identical subtrees at different paths
    /// are one node, and the same node can arrive twice in one batch.
    pub fn parse(envelopes: Vec<HgManifestEnvelope>) -> Result<Self> {
        let trees = envelopes
            .into_iter()
            .map(|envelope| {
                let node_id = envelope.node_id();
                let manifest = HgBlobManifest::parse(envelope).with_context(|| {
                    format!("parsing uploaded Mercurial manifest for tree {node_id}")
                })?;
                anyhow::Ok(ParsedTree { node_id, manifest })
            })
            .collect::<Result<Vec<_>>>()?;

        let index_by_node: HashMap<HgNodeHash, usize> = trees
            .iter()
            .enumerate()
            .map(|(index, tree)| (tree.node_id, index))
            .collect();

        let (contains, children) = trees
            .iter()
            .map(|tree| {
                child_directories(&tree.manifest).fold(
                    (Vec::new(), TreeChildren::default()),
                    |(mut contains, mut children), (name, node_id)| {
                        match index_by_node.get(&node_id) {
                            Some(&index) => {
                                contains.push(index);
                                children.in_batch.push((name.clone(), node_id));
                            }
                            None => children.external.push((name.clone(), node_id)),
                        }
                        (contains, children)
                    },
                )
            })
            .unzip();

        Ok(Self {
            trees,
            contains,
            children,
        })
    }

    /// The batch ordered so that every tree comes after the trees it contains.
    pub fn in_build_order(&self) -> impl ExactSizeIterator<Item = OrderedTree<'_>> {
        bottom_up_order(&self.contains)
            .into_iter()
            .map(|index| OrderedTree {
                node_id: self.trees[index].node_id,
                manifest: &self.trees[index].manifest,
                children: &self.children[index],
            })
    }
}

/// Order the batch so that every tree comes after the trees it contains. A node
/// is marked seen when pushed, not when finished, so a cyclic input terminates.
fn bottom_up_order(child_indices: &[Vec<usize>]) -> Vec<usize> {
    let mut seen = vec![false; child_indices.len()];
    let mut order = Vec::with_capacity(child_indices.len());
    for root in 0..child_indices.len() {
        if std::mem::replace(&mut seen[root], true) {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        while let Some((node, child_position)) = stack.pop() {
            match child_indices[node].get(child_position) {
                Some(&child) => {
                    stack.push((node, child_position + 1));
                    if !std::mem::replace(&mut seen[child], true) {
                        stack.push((child, 0));
                    }
                }
                None => order.push(node),
            }
        }
    }
    order
}

/// Build and store the augmented manifest for one uploaded tree.
///
/// Every directory inside the tree must already be derived; a missing one is an
/// error, since reporting success having built nothing would leave exactly the
/// coverage hole this path exists to close.
pub async fn build_augmented_manifest_for_uploaded_tree(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    envelope: &HgManifestEnvelope,
) -> Result<BuiltTree> {
    let manifest =
        HgBlobManifest::parse(envelope.clone()).context("parsing uploaded Mercurial manifest")?;
    let children = load_children(ctx, blobstore, &manifest).await?;
    let acl = build_acl_node(ctx, blobstore, restricted_paths, &manifest, &children).await?;

    let directories = children
        .iter()
        .map(|(name, child)| (name.clone(), child.directory.clone()))
        .collect();
    let directory = derive_augmented_manifest_for_uploaded_tree(
        ctx,
        blobstore,
        &manifest,
        &directories,
        acl.as_ref().map(|entry| entry.id),
    )
    .await?;

    Ok(BuiltTree { directory, acl })
}

/// Resolve every child directory from its own stored augmented manifest, keyed
/// on the hg node id the uploaded bytes record for it.
async fn load_children(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    manifest: &HgBlobManifest,
) -> Result<HashMap<MPathElement, ChildNode>> {
    let wanted: Vec<(MPathElement, HgNodeHash)> = manifest
        .content()
        .files
        .iter()
        .filter_map(|(name, entry)| match entry {
            Entry::Tree(id) => Some((name.clone(), id.into_nodehash())),
            Entry::Leaf(_) => None,
        })
        .collect();

    // The futures are materialised before the stream: a closure that borrows
    // `ctx` and `blobstore` inlined into `stream::iter` cannot be inferred as
    // higher-ranked, and the whole upload chain then fails to prove `Send`.
    let lookups: Vec<_> = wanted
        .into_iter()
        .map(|(name, node_id)| async move {
            let envelope = HgAugmentedManifestEnvelope::load(
                ctx,
                blobstore,
                HgAugmentedManifestId::new(node_id),
            )
            .await?
            .ok_or_else(|| anyhow!("child directory {name} ({node_id}) is not derived yet"))?;
            let acl = envelope.augmented_manifest.acl_manifest_directory_id;
            anyhow::Ok((
                name,
                ChildNode {
                    directory: HgAugmentedDirectoryNode {
                        treenode: envelope.augmented_manifest.hg_node_id,
                        augmented_manifest_id: envelope.augmented_manifest_id,
                        augmented_manifest_size: envelope.augmented_manifest_size,
                        acl_manifest_directory_id: acl,
                    },
                    // The envelope records the pointer but not the flags, so the
                    // ACL builder recovers them from the child's rollup data.
                    acl: acl.map(AclChildNode::IdOnly),
                },
            ))
        })
        .collect();

    stream::iter(lookups)
        .buffer_unordered(MAX_CONCURRENT_CHILD_LOOKUPS)
        .try_collect()
        .await
}

async fn build_acl_node(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    manifest: &HgBlobManifest,
    children: &HashMap<MPathElement, ChildNode>,
) -> Result<Option<AclManifestDirectoryEntry>> {
    let acl_file_name = restricted_paths.config().acl_file_name().to_string();
    let acl_file_element = MPathElement::new(acl_file_name.as_bytes().to_vec())?;
    let own_acl_file =
        manifest
            .content()
            .files
            .get(&acl_file_element)
            .and_then(|entry| match entry {
                Entry::Leaf((_, filenode_id)) => Some(*filenode_id),
                Entry::Tree(_) => None,
            });
    // The ACL manifest is sparse, and the node is a function of this tree's own
    // contents, so with no ACL file here and no child carrying a node there is
    // provably nothing to build.
    if own_acl_file.is_none() && children.values().all(|child| child.acl.is_none()) {
        return Ok(None);
    }

    let own_acl_file = match own_acl_file {
        Some(filenode_id) => Some(filenode_id.load(ctx, blobstore).await?.content_id()),
        None => None,
    };
    acl_node_for_directory(
        ctx,
        blobstore,
        DirectoryAclInputs {
            acl_file_name: &acl_file_name,
            own_acl_file,
            children: children
                .iter()
                .filter_map(|(name, child)| child.acl.clone().map(|acl| (name.clone(), acl)))
                .collect(),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use mercurial_types::HgManifestEnvelopeMut;
    use mononoke_macros::mononoke;

    use super::*;

    fn node(byte: u8) -> HgNodeHash {
        HgNodeHash::from_bytes(&[byte; 20]).expect("twenty bytes is a node hash")
    }

    /// A manifest listing `children` as subdirectories, plus one file so the
    /// leaf branch of `child_directories` is exercised rather than assumed.
    fn tree(node_id: HgNodeHash, children: &[(&str, HgNodeHash)]) -> HgManifestEnvelope {
        let mut lines: Vec<String> = children
            .iter()
            .map(|(name, child)| format!("{name}\0{child}t\n"))
            .collect();
        lines.push(format!("file\0{node_id}\n"));
        lines.sort();

        HgManifestEnvelopeMut {
            node_id,
            p1: None,
            p2: None,
            computed_node_id: node_id,
            contents: Bytes::from(lines.concat()),
        }
        .freeze()
    }

    fn node_ids(children: &[(MPathElement, HgNodeHash)]) -> Vec<HgNodeHash> {
        children.iter().map(|(_, node_id)| *node_id).collect()
    }

    #[mononoke::test]
    fn test_parse_splits_children_by_whether_they_arrived() {
        // foo contains bar, bar contains baz, and only foo and bar were
        // uploaded.
        let (foo, bar, baz) = (node(1), node(2), node(3));
        let batch =
            UploadedTreeBatch::parse(vec![tree(foo, &[("bar", bar)]), tree(bar, &[("baz", baz)])])
                .expect("the fixture batch parses");

        let ordered: Vec<_> = batch.in_build_order().collect();
        assert_eq!(
            ordered.iter().map(|tree| tree.node_id).collect::<Vec<_>>(),
            vec![bar, foo],
            "bar is built before the foo that contains it"
        );

        assert_eq!(
            node_ids(&ordered[0].children.external),
            vec![baz],
            "baz did not arrive, so it has to come from storage"
        );
        assert_eq!(
            node_ids(&ordered[1].children.in_batch),
            vec![bar],
            "bar arrived with the batch, so foo needs nothing from storage"
        );
        assert!(
            ordered[1].children.external.is_empty(),
            "foo's only child directory arrived with it"
        );
    }

    #[mononoke::test]
    fn test_bottom_up_order_puts_children_first() {
        // 0 contains 1 and 2; 1 contains 3.
        let contains = vec![vec![1, 2], vec![3], vec![], vec![]];
        let order = bottom_up_order(&contains);
        let position = |node: usize| order.iter().position(|n| *n == node).expect("visited");
        assert_eq!(order.len(), 4, "every tree is visited exactly once");
        assert!(position(3) < position(1), "3 is inside 1");
        assert!(position(1) < position(0), "1 is inside 0");
        assert!(position(2) < position(0), "2 is inside 0");
    }

    #[mononoke::test]
    fn test_bottom_up_order_terminates_on_a_cycle() {
        // Not reachable from a content-addressed manifest, but must not hang.
        let contains = vec![vec![1], vec![0]];
        assert_eq!(bottom_up_order(&contains).len(), 2);
    }
}
