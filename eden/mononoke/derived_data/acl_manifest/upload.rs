/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Build one directory's ACL manifest node from that directory's own contents.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use itertools::Either;
use manifest::Entry;
use manifest::TreeInfoSubentries;
use mononoke_types::ContentId;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestDirectoryEntry;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::typed_hash::AclManifestId;

use crate::derive::AclManifestNodeInfo;
use crate::derive::create_acl_manifest;
use crate::derive::fetch_and_parse_acl_file;
use crate::derive::store_acl_entry_from_acl_file;

/// A child directory that has an ACL manifest node.
#[derive(Clone, Debug)]
pub enum AclChildNode {
    /// The child's entry as its producer recorded it: a sibling built earlier
    /// in the same batch, or the parent commit's ACL node for a child this
    /// commit did not touch.
    Known(AclManifestDirectoryEntry),
    /// Only the child's id is known. Its flags are recovered by loading it,
    /// which reads rollup data rather than enumerating its subentries.
    IdOnly(AclManifestId),
}

impl AclChildNode {
    fn split(self) -> (AclManifestId, Option<AclManifestNodeInfo>) {
        match self {
            Self::Known(entry) => (
                entry.id,
                Some(AclManifestNodeInfo {
                    is_restricted: entry.is_restricted,
                    has_restricted_descendants: entry.has_restricted_descendants,
                }),
            ),
            Self::IdOnly(id) => (id, None),
        }
    }
}

/// One directory's ACL-relevant contents as they stand in the tree being built.
///
/// This is a statement of the directory's state, not a delta against a parent.
/// A child that is absent here is absent from the node, which is what makes a
/// deleted restricted subtree impossible to carry forward by accident.
pub struct DirectoryAclInputs<'a> {
    /// The configured ACL file name, e.g. `.slacl`.
    pub acl_file_name: &'a str,
    /// The directory's own ACL file, if it has one. Content that does not parse
    /// is logged and treated as absent, leaving the directory unrestricted.
    pub own_acl_file: Option<ContentId>,
    /// Every child directory of this directory that has an ACL node. A child
    /// named `acl_file_name` is rejected rather than built.
    pub children: BTreeMap<MPathElement, AclChildNode>,
}

/// Build the ACL manifest node for a single directory, from inputs local to it.
///
/// Returns `None` when the directory has neither a restriction nor a restricted
/// descendant, which is the absence the canonical derivation records for the
/// same directory.
pub async fn acl_node_for_directory(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    dir: DirectoryAclInputs<'_>,
) -> Result<Option<AclManifestDirectoryEntry>> {
    let DirectoryAclInputs {
        acl_file_name,
        own_acl_file,
        children,
    } = dir;

    let mut subentries: TreeInfoSubentries<
        AclManifestId,
        AclManifestRestriction,
        AclManifestNodeInfo,
        LoadableShardedMapV2Node<AclManifestEntry>,
    > = BTreeMap::new();

    if let Some(content_id) = own_acl_file {
        let acl_file_path = NonRootMPath::new(acl_file_name)
            .with_context(|| format!("building acl path from file name {acl_file_name}"))?;
        let parsed =
            fetch_and_parse_acl_file(ctx, blobstore, content_id, &acl_file_path, acl_file_name)
                .await
                .with_context(|| format!("parsing acl file {acl_file_name}"))?;
        if let Some(acl_file) = parsed {
            let entry_blob_id = store_acl_entry_from_acl_file(ctx, blobstore, &acl_file).await?;
            let name = MPathElement::new(acl_file_name.as_bytes().to_vec())
                .with_context(|| format!("building acl element from file name {acl_file_name}"))?;
            subentries.insert(
                name.to_smallvec(),
                Either::Left((None, Entry::Leaf(AclManifestRestriction { entry_blob_id }))),
            );
        }
    }

    for (name, child) in children {
        // A directory at the ACL file's own name would overwrite the leaf
        // inserted above, and the restriction lookup would then find a
        // directory and report the node unrestricted. A manifest name is
        // either a file or a directory, so this can only be a caller error.
        if name.as_ref() == acl_file_name.as_bytes() {
            bail!("child directory {name} collides with acl file name {acl_file_name}");
        }

        let (id, info) = child.split();
        // The canonical empty id is how absence is spelled everywhere else;
        // pinning it here would invent a phantom restricted child.
        if id == AclManifest::empty_id() {
            continue;
        }
        subentries.insert(name.to_smallvec(), Either::Left((info, Entry::Tree(id))));
    }

    let (info, id) = create_acl_manifest(
        ctx.clone(),
        blobstore.clone(),
        acl_file_name.as_bytes(),
        subentries,
    )
    .await
    .with_context(|| format!("building acl node for directory with acl file {acl_file_name}"))?;

    if id == AclManifest::empty_id() {
        return Ok(None);
    }

    Ok(Some(AclManifestDirectoryEntry {
        id,
        is_restricted: info.is_restricted,
        has_restricted_descendants: info.has_restricted_descendants,
    }))
}
