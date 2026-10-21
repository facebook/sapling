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
use std::sync::Mutex;

use acl_manifest::AclChildNode;
use acl_manifest::DirectoryAclInputs;
use acl_manifest::acl_node_for_directory;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use blobstore::Storable;
use bounded_traversal::bounded_traversal_dag;
use context::CoreContext;
use either::Either;
use filestore::FetchKey;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use manifest::Entry;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestEnvelope;
use mercurial_types::HgNodeHash;
use mercurial_types::ShardedHgAugmentedManifest;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::sharded_augmented_manifest::HgAugmentedDirectoryNode;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mononoke_types::FileType;
use mononoke_types::MPathElement;
use mononoke_types::TrieMap;
use mononoke_types::acl_manifest::AclManifestDirectoryEntry;
use mononoke_types::acl_manifest::AclManifestDirectoryRestriction;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::typed_hash::AclManifestId;
use restricted_paths_common::RestrictedPathsConfigBased;
use thiserror::Error;

use crate::derive_hg_augmented_manifest::validate_augmented_manifest_element;

/// Per tree build, so it multiplies with `MAX_CONCURRENT_TREE_BUILDS`.
const MAX_CONCURRENT_CHILD_LOOKUPS: usize = 100;
const MAX_CONCURRENT_TREE_BUILDS: usize = 10;
const MAX_CONCURRENT_ENVELOPE_STORES: usize = 100;

#[derive(Debug, Error)]
pub enum UploadTreeBuildError {
    /// A tree was uploaded before a child it contains.
    #[error(
        "tree {tree} contains {name} ({child}), which is neither built in this batch nor already derived"
    )]
    MissingChild {
        tree: HgNodeHash,
        name: MPathElement,
        child: HgNodeHash,
    },
}

#[derive(Debug, Clone)]
pub enum DirectoryAcl {
    /// No ACL file and no child with an ACL node, so nothing was built.
    NotNeeded,
    /// Built but came to nothing, e.g. an ACL file that does not parse.
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
    pub acl: DirectoryAcl,
}

#[derive(Debug)]
pub struct UploadTreeAugmented {
    pub node_id: HgNodeHash,
    pub acl: DirectoryAcl,
    /// 0 for a tree containing no other batch tree, else one above its highest child.
    pub level: usize,
    pub envelope: HgAugmentedManifestEnvelope,
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

#[derive(Default)]
pub struct TreeChildren {
    pub in_batch: Vec<(MPathElement, HgNodeHash)>,
    /// Not in the batch, so they must already be derived.
    pub external: Vec<(MPathElement, HgNodeHash)>,
}

pub struct UploadedTreeBatch {
    trees: Vec<ParsedTree>,
    /// Per tree, the positions of the batch trees it contains.
    contains: Vec<Vec<usize>>,
    children: Vec<TreeChildren>,
}

impl UploadedTreeBatch {
    /// Edges are node ids, not paths: identical subtrees are one node.
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

    fn top_trees(&self) -> Vec<usize> {
        let mut contained = vec![false; self.trees.len()];
        for &child in self.contains.iter().flatten() {
            contained[child] = true;
        }
        (0..self.trees.len())
            .filter(|&index| !contained[index])
            .collect()
    }
}

struct ChildSources<'a> {
    children: &'a TreeChildren,
    /// Trees built earlier in the same batch.
    siblings: &'a HashMap<HgNodeHash, &'a BuiltTree>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BuildNode {
    /// Above the batch's top trees, since the traversal needs a single start.
    Root,
    Tree(usize),
}

/// What a built tree hands up to the trees that contain it.
struct AugmentedTree {
    node_id: HgNodeHash,
    tree: BuiltTree,
    level: usize,
}

/// Build an augmented manifest for every uploaded tree, children before
/// parents, without storing any. A child neither in the batch nor already
/// derived fails the whole batch.
pub async fn build_augmented_manifests_for_uploaded_trees(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    trees: Vec<HgManifestEnvelope>,
) -> Result<Vec<UploadTreeAugmented>> {
    let batch = UploadedTreeBatch::parse(trees)?;
    let top_trees: Vec<BuildNode> = batch.top_trees().into_iter().map(BuildNode::Tree).collect();
    // By batch position, so the output order does not depend on build order.
    let built: Mutex<Vec<Option<UploadTreeAugmented>>> =
        Mutex::new(batch.trees.iter().map(|_| None).collect());

    let (batch, built_ref) = (&batch, &built);
    let traversed = bounded_traversal_dag(
        MAX_CONCURRENT_TREE_BUILDS,
        BuildNode::Root,
        move |node| {
            let children = match node {
                BuildNode::Root => top_trees.clone(),
                BuildNode::Tree(index) => batch.contains[index]
                    .iter()
                    .map(|&child| BuildNode::Tree(child))
                    .collect(),
            };
            future::ok((node, children)).boxed()
        },
        move |node, children: bounded_traversal::Iter<Option<Arc<AugmentedTree>>>| {
            let children: Vec<Arc<AugmentedTree>> = children.flatten().collect();
            async move {
                let BuildNode::Tree(index) = node else {
                    return Ok(None);
                };
                let tree = &batch.trees[index];
                let siblings = children
                    .iter()
                    .map(|child| (child.node_id, &child.tree))
                    .collect();
                let (result, envelope) = build_uploaded_tree(
                    ctx,
                    blobstore,
                    restricted_paths,
                    &tree.manifest,
                    ChildSources {
                        children: &batch.children[index],
                        siblings: &siblings,
                    },
                )
                .await
                .with_context(|| {
                    format!(
                        "building the augmented manifest for uploaded tree {}",
                        tree.node_id
                    )
                })?;
                let level = children
                    .iter()
                    .map(|child| child.level + 1)
                    .max()
                    .unwrap_or(0);
                built_ref
                    .lock()
                    .expect("should not be poisoned, nothing panics while holding it")[index] =
                    Some(UploadTreeAugmented {
                        node_id: tree.node_id,
                        acl: result.acl.clone(),
                        level,
                        envelope,
                    });
                anyhow::Ok(Some(Arc::new(AugmentedTree {
                    node_id: tree.node_id,
                    tree: result,
                    level,
                })))
            }
            .boxed()
        },
    )
    .await?;

    traversed.context("the uploaded batch contains a cycle")?;
    built
        .into_inner()
        .expect("should not be poisoned, nothing panics while holding it")
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .context("the uploaded batch contains a cycle")
}

/// Store the built envelopes, concurrently and in no order.
pub async fn store_uploaded_tree_envelopes(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    built: &[UploadTreeAugmented],
) -> Result<()> {
    // Collected first: inlined into `stream::iter` the closure is not
    // higher-ranked and callers cannot prove `Send`.
    let stores: Vec<_> = built
        .iter()
        .map(|tree| tree.envelope.clone().store(ctx, blobstore))
        .collect();
    stream::iter(stores)
        .buffer_unordered(MAX_CONCURRENT_ENVELOPE_STORES)
        .try_for_each(|_| future::ok(()))
        .await
}

async fn build_uploaded_tree(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    manifest: &HgBlobManifest,
    sources: ChildSources<'_>,
) -> Result<(BuiltTree, HgAugmentedManifestEnvelope)> {
    let parent = parent_entries(ctx, blobstore, manifest.p1()).await?;
    let (children, acl) =
        children_and_acl(ctx, blobstore, restricted_paths, manifest, sources, &parent).await?;
    let acl_overlay = acl.node().map(|entry| entry.id);

    let files = &manifest.content().files;

    let mut subentries = TrieMap::default();
    let mut to_build: Vec<(MPathElement, FileType, HgFileNodeId)> = Vec::new();
    for (name, entry) in files.iter() {
        let Entry::Leaf((file_type, filenode_id)) = entry else {
            continue;
        };
        validate_augmented_manifest_element(name.as_ref())?;
        // A leaf depends only on its filenode and file type, so a wrong p1
        // can only cost a miss.
        match parent.leaves.get(name) {
            Some(leaf)
                if leaf.filenode == filenode_id.into_nodehash() && leaf.file_type == *file_type =>
            {
                subentries.insert(
                    name.clone(),
                    Either::Left(HgAugmentedManifestEntry::FileNode(leaf.clone())),
                );
            }
            _ => to_build.push((name.clone(), *file_type, *filenode_id)),
        }
    }

    let leaf_futures: Vec<_> = to_build
        .into_iter()
        .map(|(name, file_type, filenode_id)| async move {
            let leaf = build_uploaded_file_leaf(ctx, blobstore, file_type, filenode_id).await?;
            anyhow::Ok((name, HgAugmentedManifestEntry::FileNode(leaf)))
        })
        .collect();
    let leaves = stream::iter(leaf_futures)
        .buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;

    for (name, entry) in leaves {
        subentries.insert(name, Either::Left(entry));
    }
    for (name, entry) in files.iter() {
        let Entry::Tree(id) = entry else { continue };
        let child = children
            .get(name)
            .map(|child| &child.directory)
            .ok_or_else(|| {
                anyhow!(
                    "uploaded tree {} names child directory {name} ({}) with no augmented manifest",
                    manifest.node_id(),
                    id.into_nodehash(),
                )
            })?;
        validate_augmented_manifest_element(name.as_ref())?;
        subentries.insert(
            name.clone(),
            Either::Left(HgAugmentedManifestEntry::DirectoryNode(child.clone())),
        );
    }

    // The header is the client's: a mirror upload's `node_id` is not the
    // content hash, and it is the key the envelope is stored and served under.
    let augmented_manifest = ShardedHgAugmentedManifest {
        hg_node_id: manifest.node_id(),
        p1: manifest.p1(),
        p2: manifest.p2(),
        computed_node_id: manifest.computed_node_id(),
        subentries: ShardedMapV2Node::from_entries_and_partial_maps(ctx, blobstore, subentries)
            .await?,
        acl_manifest_directory_id: acl_overlay,
    };
    let (augmented_manifest_id, augmented_manifest_size) = augmented_manifest
        .clone()
        .compute_content_addressed_digest(ctx, blobstore)
        .await?;
    let envelope = HgAugmentedManifestEnvelope {
        augmented_manifest_id,
        augmented_manifest_size,
        augmented_manifest,
    };
    let directory = HgAugmentedDirectoryNode {
        treenode: envelope.augmented_manifest.hg_node_id,
        augmented_manifest_id: envelope.augmented_manifest_id,
        augmented_manifest_size: envelope.augmented_manifest_size,
        acl_manifest_directory_id: acl_overlay,
    };

    Ok((BuiltTree { directory, acl }, envelope))
}

/// Clients upload content before the trees that list it, so missing metadata
/// is an error rather than a reason to recompute it.
async fn build_uploaded_file_leaf(
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    file_type: FileType,
    filenode_id: HgFileNodeId,
) -> Result<HgAugmentedFileLeafNode> {
    let filenode = filenode_id.load(ctx, blobstore).await?;
    let content_id = filenode.content_id();
    let metadata =
        filestore::get_metadata_readonly(blobstore, ctx, &FetchKey::Canonical(content_id))
            .await?
            .flatten()
            .ok_or_else(|| {
                anyhow!(
                    "missing content metadata for {content_id}; content must be uploaded before the trees that list it"
                )
            })?;
    Ok(HgAugmentedFileLeafNode {
        file_type,
        filenode: filenode_id.into_nodehash(),
        total_size: metadata.total_size,
        content_blake3: metadata.seeded_blake3,
        content_sha1: metadata.sha1,
        file_header_metadata: if filenode.metadata().is_empty() {
            None
        } else {
            Some(filenode.metadata().clone())
        },
    })
}

/// This directory's entries in p1, reused for those that did not change.
#[derive(Default)]
struct ParentEntries {
    leaves: HashMap<MPathElement, HgAugmentedFileLeafNode>,
    directories: HashMap<MPathElement, HgAugmentedDirectoryNode>,
    acl_manifest_directory_id: Option<AclManifestId>,
}

/// No p1, or a p1 never derived, builds every entry from scratch.
async fn parent_entries(
    ctx: &CoreContext,
    blobstore: &(impl KeyedBlobstore + 'static),
    p1: Option<HgNodeHash>,
) -> Result<ParentEntries> {
    let Some(p1) = p1 else {
        return Ok(ParentEntries::default());
    };
    let Some(envelope) =
        HgAugmentedManifestEnvelope::load(ctx, blobstore, HgAugmentedManifestId::new(p1)).await?
    else {
        return Ok(ParentEntries::default());
    };
    let acl_manifest_directory_id = envelope.augmented_manifest.acl_manifest_directory_id;
    envelope
        .augmented_manifest
        .subentries
        .into_entries(ctx, blobstore)
        .and_then(|(name, entry)| async move { Ok((MPathElement::from_smallvec(name)?, entry)) })
        .try_fold(
            ParentEntries {
                acl_manifest_directory_id,
                ..Default::default()
            },
            |mut parent, (name, entry)| async move {
                match entry {
                    HgAugmentedManifestEntry::FileNode(leaf) => {
                        parent.leaves.insert(name, leaf);
                    }
                    HgAugmentedManifestEntry::DirectoryNode(directory) => {
                        parent.directories.insert(name, directory);
                    }
                }
                Ok(parent)
            },
        )
        .await
}

/// Resolve the tree's child directories, then build its ACL node from them.
async fn children_and_acl(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    restricted_paths: &RestrictedPathsConfigBased,
    manifest: &HgBlobManifest,
    sources: ChildSources<'_>,
    parent: &ParentEntries,
) -> Result<(HashMap<MPathElement, ChildNode>, DirectoryAcl)> {
    let children = load_children(
        ctx,
        blobstore,
        manifest.node_id(),
        sources,
        &parent.directories,
    )
    .await?;

    let acl_file_name = restricted_paths.config().acl_file_name().to_string();
    let acl_file_element = MPathElement::new(acl_file_name.as_bytes().to_vec())?;
    let own_acl_file =
        manifest
            .content()
            .files
            .get(&acl_file_element)
            .and_then(|entry| match entry {
                Entry::Leaf(leaf) => Some(*leaf),
                Entry::Tree(_) => None,
            });

    let acl = if own_acl_file.is_none() && children.values().all(|child| child.acl.is_none()) {
        DirectoryAcl::NotNeeded
    } else if let Some(acl) = reuse_parent_acl(
        ctx,
        blobstore,
        parent,
        &acl_file_element,
        own_acl_file,
        &children,
    )
    .await?
    {
        acl
    } else {
        let own_acl_file = match own_acl_file {
            Some((_, filenode_id)) => Some(filenode_id.load(ctx, blobstore).await?.content_id()),
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

    Ok((children, acl))
}

/// p1's ACL node, when the own ACL file and the child pointers are unchanged.
/// Canonical derivation never re-parses that file either.
async fn reuse_parent_acl(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    parent: &ParentEntries,
    acl_file_element: &MPathElement,
    own_acl_file: Option<(FileType, HgFileNodeId)>,
    children: &HashMap<MPathElement, ChildNode>,
) -> Result<Option<DirectoryAcl>> {
    let same_acl_file = match (own_acl_file, parent.leaves.get(acl_file_element)) {
        (None, None) => true,
        (Some((file_type, filenode_id)), Some(leaf)) => {
            leaf.filenode == filenode_id.into_nodehash() && leaf.file_type == file_type
        }
        _ => false,
    };
    let parent_pointer = |name| {
        parent
            .directories
            .get(name)
            .and_then(|directory| directory.acl_manifest_directory_id)
    };
    let same_children = children
        .iter()
        .all(|(name, child)| child.directory.acl_manifest_directory_id == parent_pointer(name))
        && parent.directories.iter().all(|(name, directory)| {
            directory.acl_manifest_directory_id.is_none() || children.contains_key(name)
        });
    if !same_acl_file || !same_children {
        return Ok(None);
    }

    match (parent.acl_manifest_directory_id, own_acl_file) {
        (Some(id), _) => {
            let node = id.load(ctx, blobstore).await?;
            Ok(Some(DirectoryAcl::Node(AclManifestDirectoryEntry {
                id,
                is_restricted: matches!(
                    node.restriction,
                    AclManifestDirectoryRestriction::Restricted(_)
                ),
                has_restricted_descendants: node.subentries.rollup_data().has_restricted,
            })))
        }
        // The ACL file did not parse when it last changed.
        (None, Some(_)) => Ok(Some(DirectoryAcl::Empty)),
        (None, None) => Ok(None),
    }
}

/// Each child comes from the batch, from p1 when its node id is unchanged, or
/// from its stored augmented manifest.
async fn load_children(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    tree: HgNodeHash,
    sources: ChildSources<'_>,
    parent_directories: &HashMap<MPathElement, HgAugmentedDirectoryNode>,
) -> Result<HashMap<MPathElement, ChildNode>> {
    let resolved = sources
        .children
        .in_batch
        .iter()
        .map(|(name, node_id)| {
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
                    acl: sibling.acl.node().cloned().map(AclChildNode::Known),
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    let (unchanged, changed): (Vec<_>, Vec<_>) =
        sources
            .children
            .external
            .iter()
            .partition(|(name, node_id)| {
                parent_directories
                    .get(name)
                    .is_some_and(|directory| directory.treenode == *node_id)
            });
    let reused = unchanged.into_iter().filter_map(|(name, _)| {
        let directory = parent_directories.get(name)?;
        Some((
            name.clone(),
            ChildNode {
                directory: directory.clone(),
                acl: directory
                    .acl_manifest_directory_id
                    .map(AclChildNode::IdOnly),
            },
        ))
    });

    let lookups: Vec<_> = changed
        .into_iter()
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
                    acl: acl.map(AclChildNode::IdOnly),
                },
            ))
        })
        .collect();

    let fetched: Vec<(MPathElement, ChildNode)> = stream::iter(lookups)
        .buffer_unordered(MAX_CONCURRENT_CHILD_LOOKUPS)
        .try_collect()
        .await?;

    Ok(resolved.into_iter().chain(reused).chain(fetched).collect())
}
