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
