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
use thiserror::Error;

use crate::derive_hg_augmented_manifest::derive_augmented_manifest_for_uploaded_tree;

const MAX_CONCURRENT_CHILD_LOOKUPS: usize = 100;
/// Each tree build fans out its own child lookups, so this multiplies with
/// `MAX_CONCURRENT_CHILD_LOOKUPS`.
const MAX_CONCURRENT_TREE_BUILDS: usize = 10;

/// Why an uploaded batch could not be built.
#[derive(Debug, Error)]
pub enum UploadTreeBuildError {
    /// The client's fault: a parent uploaded before a child it contains.
    ///
    /// Phrased for both callers: an out-of-batch child with no stored envelope,
    /// and an in-batch child the ordering could not put first.
    #[error(
        "tree {tree} contains {name} ({child}), which is neither built in this batch nor already derived"
    )]
    MissingChild {
        tree: HgNodeHash,
        name: MPathElement,
        child: HgNodeHash,
    },
}

/// What the ACL manifest pass made of one directory.
#[derive(Debug, Clone)]
pub enum DirectoryAcl {
    /// No ACL file here and no child carrying a node, so there was provably
    /// nothing to build. The sparse common case.
    NotNeeded,
    /// Built and came to nothing: an ACL file that does not parse leaves the
    /// directory unrestricted, and an empty child node is not carried.
    Empty,
    Node(AclManifestDirectoryEntry),
}

impl DirectoryAcl {
    fn node(&self) -> Option<&AclManifestDirectoryEntry> {
        match self {
            Self::Node(entry) => Some(entry),
            Self::NotNeeded | Self::Empty => None,
        }
    }
}

#[derive(Debug)]
pub struct BuiltTree {
    pub directory: HgAugmentedDirectoryNode,
    /// Carries the node's flags as well as its id, so the containing directory
    /// can list this tree as a child without loading it back.
    pub acl: DirectoryAcl,
}

/// One tree of a batch, once built.
#[derive(Debug)]
pub struct UploadTreeAugmented {
    pub node_id: HgNodeHash,
    pub acl: DirectoryAcl,
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
#[derive(Clone, Copy)]
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

    /// The batch grouped into levels, each containing only trees whose
    /// in-batch children all sit in earlier levels.
    pub fn in_build_levels(&self) -> Vec<Vec<OrderedTree<'_>>> {
        build_levels(&self.contains)
            .into_iter()
            .map(|level| {
                level
                    .into_iter()
                    .map(|index| OrderedTree {
                        node_id: self.trees[index].node_id,
                        manifest: &self.trees[index].manifest,
                        children: &self.children[index],
                    })
                    .collect()
            })
            .collect()
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

/// Group the batch by height: a tree with no in-batch children is at level 0,
/// and any other is one above its highest child.
///
/// On a cyclic input some edge cannot point to a strictly lower level, since
/// heights cannot strictly decrease all the way round a cycle. That child is
/// then not yet built when its parent is, so the batch fails rather than
/// building against a missing child.
fn build_levels(child_indices: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut height = vec![0usize; child_indices.len()];
    let mut levels: Vec<Vec<usize>> = Vec::new();
    for node in bottom_up_order(child_indices) {
        height[node] = child_indices[node]
            .iter()
            .map(|&child| height[child] + 1)
            .max()
            .unwrap_or(0);
        if levels.len() <= height[node] {
            levels.resize_with(height[node] + 1, Vec::new);
        }
        levels[height[node]].push(node);
    }
    levels
}

/// Where the children of the tree being built are resolved from.
struct ChildSources<'a> {
    children: &'a TreeChildren,
    /// Trees built earlier in the same batch. What the map adds over a
    /// blobstore lookup is their ACL flags, which the envelope does not record.
    siblings: &'a HashMap<HgNodeHash, BuiltTree>,
}

/// Build and store the augmented manifest for one uploaded tree. Every
/// directory inside it must already be derived; a missing one is an error.
pub async fn build_augmented_manifest_for_uploaded_tree(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    envelope: &HgManifestEnvelope,
) -> Result<BuiltTree> {
    // A batch of one, so every child directory is external by construction.
    let batch = UploadedTreeBatch::parse(vec![envelope.clone()])?;
    let tree = *batch
        .in_build_levels()
        .first()
        .and_then(|level| level.first())
        .context("a batch of one uploaded tree has one tree to build")?;
    build_uploaded_tree(
        ctx,
        blobstore,
        restricted_paths,
        tree.manifest,
        ChildSources {
            children: tree.children,
            siblings: &HashMap::new(),
        },
    )
    .await
}

/// Build and store an augmented manifest for every uploaded tree, bottom-up and
/// a level at a time. A child neither in the batch nor already derived fails
/// the whole batch.
pub async fn build_augmented_manifests_for_uploaded_trees(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    trees: Vec<HgManifestEnvelope>,
) -> Result<Vec<UploadTreeAugmented>> {
    let batch = UploadedTreeBatch::parse(trees)?;

    let levels = batch.in_build_levels();
    let tree_count = levels.iter().map(Vec::len).sum();

    let mut built: HashMap<HgNodeHash, BuiltTree> = HashMap::with_capacity(tree_count);
    let mut augmented = Vec::with_capacity(tree_count);
    for level in levels {
        let siblings = &built;
        let builds: Vec<_> = level
            .into_iter()
            .map(|tree| async move {
                let result = build_uploaded_tree(
                    ctx,
                    blobstore,
                    restricted_paths,
                    tree.manifest,
                    ChildSources {
                        children: tree.children,
                        siblings,
                    },
                )
                .await
                .with_context(|| {
                    format!(
                        "building the augmented manifest for uploaded tree {}",
                        tree.node_id
                    )
                })?;
                anyhow::Ok((tree.node_id, result))
            })
            .collect();
        let level_built: Vec<(HgNodeHash, BuiltTree)> = stream::iter(builds)
            .buffered(MAX_CONCURRENT_TREE_BUILDS)
            .try_collect()
            .await?;
        for (node_id, result) in level_built {
            augmented.push(UploadTreeAugmented {
                node_id,
                acl: result.acl.clone(),
            });
            built.insert(node_id, result);
        }
    }

    Ok(augmented)
}

async fn build_uploaded_tree(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    manifest: &HgBlobManifest,
    sources: ChildSources<'_>,
) -> Result<BuiltTree> {
    let children = load_children(ctx, blobstore, manifest.node_id(), sources).await?;

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

    let acl = if own_acl_file.is_none() && children.values().all(|child| child.acl.is_none()) {
        DirectoryAcl::NotNeeded
    } else {
        let own_acl_file = match own_acl_file {
            Some(filenode_id) => Some(filenode_id.load(ctx, blobstore).await?.content_id()),
            None => None,
        };
        let node = acl_node_for_directory(
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
        .await?;
        match node {
            Some(entry) => DirectoryAcl::Node(entry),
            None => DirectoryAcl::Empty,
        }
    };

    let directories = children
        .iter()
        .map(|(name, child)| (name.clone(), child.directory.clone()))
        .collect();
    let directory = derive_augmented_manifest_for_uploaded_tree(
        ctx,
        blobstore,
        manifest,
        &directories,
        acl.node().map(|entry| entry.id),
    )
    .await?;

    Ok(BuiltTree { directory, acl })
}

/// Resolve every child directory, from the batch when it arrived there and from
/// its own stored augmented manifest otherwise.
async fn load_children(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    tree: HgNodeHash,
    sources: ChildSources<'_>,
) -> Result<HashMap<MPathElement, ChildNode>> {
    let resolved = sources
        .children
        .in_batch
        .iter()
        .map(|(name, node_id)| {
            // Only reachable if the batch is not a DAG, since levelling
            // otherwise builds every in-batch child before its parent.
            let sibling = sources.siblings.get(node_id).ok_or_else(|| {
                UploadTreeBuildError::MissingChild {
                    tree,
                    name: name.clone(),
                    child: *node_id,
                }
            })?;
            anyhow::Ok((
                name.clone(),
                ChildNode {
                    directory: sibling.directory.clone(),
                    // Built moments ago, so its flags are known.
                    acl: sibling.acl.node().cloned().map(AclChildNode::Known),
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    // Materialised before the stream: a closure borrowing `ctx` inlined into
    // `stream::iter` is not inferred higher-ranked, and `Send` then fails.
    let lookups: Vec<_> = sources
        .children
        .external
        .iter()
        .map(|(name, node_id)| async move {
            let envelope = HgAugmentedManifestEnvelope::load(
                ctx,
                blobstore,
                HgAugmentedManifestId::new(*node_id),
            )
            .await?
            .ok_or_else(|| UploadTreeBuildError::MissingChild {
                tree,
                name: name.clone(),
                child: *node_id,
            })?;
            let acl = envelope.augmented_manifest.acl_manifest_directory_id;
            anyhow::Ok((
                name.clone(),
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

    let fetched: Vec<(MPathElement, ChildNode)> = stream::iter(lookups)
        .buffer_unordered(MAX_CONCURRENT_CHILD_LOOKUPS)
        .try_collect()
        .await?;

    Ok(resolved.into_iter().chain(fetched).collect())
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

        let ordered: Vec<_> = batch.in_build_levels().into_iter().flatten().collect();
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
    fn test_build_levels_groups_trees_by_height() {
        // 0 contains 1 and 2; 1 contains 3.
        let contains = vec![vec![1, 2], vec![3], vec![], vec![]];
        let mut levels = build_levels(&contains);
        levels.iter_mut().for_each(|level| level.sort());
        assert_eq!(
            levels,
            vec![vec![2, 3], vec![1], vec![0]],
            "2 and 3 contain nothing in the batch, so they build together first"
        );
    }

    #[mononoke::test]
    fn test_build_levels_leaves_a_cycle_edge_unsatisfied() {
        // Not reachable from a content-addressed manifest. Building must then
        // fail on a missing child, which needs an edge whose child is not in a
        // strictly lower level than its parent.
        let contains = vec![vec![1], vec![2], vec![0]];
        let levels = build_levels(&contains);
        let level_of = |node: usize| {
            levels
                .iter()
                .position(|level| level.contains(&node))
                .expect("every tree is placed")
        };
        assert_eq!(levels.iter().map(Vec::len).sum::<usize>(), 3);
        assert!(
            contains
                .iter()
                .enumerate()
                .any(|(parent, children)| children
                    .iter()
                    .any(|&child| level_of(child) >= level_of(parent))),
            "some child is not built before its parent"
        );
    }

    #[mononoke::test]
    fn test_bottom_up_order_terminates_on_a_cycle() {
        // Not reachable from a content-addressed manifest, but must not hang.
        let contains = vec![vec![1], vec![0]];
        assert_eq!(bottom_up_order(&contains).len(), 2);
    }
}
