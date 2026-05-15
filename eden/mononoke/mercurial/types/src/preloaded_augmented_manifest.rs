/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use blobstore::KeyedBlobstore;
use bytes::Bytes;
use context::CoreContext;
use futures::TryStreamExt;
use futures::future;
use mononoke_types::MPathElement;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestDirectoryRestriction;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::hash::Blake3;

use crate::HgAugmentedManifestEntry;
use crate::HgAugmentedManifestEnvelope;
/// This is temporary type to preload Augmented Manifest and build manifest blobs in sapling native format
/// The type will be used to convert an HgAugmentedManifest entry into an SaplingRemoteApi TreeEntry.
use crate::HgNodeHash;
use crate::manifest::Type as HgManifestType;

pub type HgAugmentedManifestMetadata = HgAugmentedManifestEntry;

/// A mutable representation of a Mercurial manifest node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgPreloadedAugmentedManifest {
    pub hg_node_id: HgNodeHash,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub augmented_manifest_id: Blake3,
    pub augmented_manifest_size: u64,
    pub contents: Bytes,
    pub children_augmented_metadata: Vec<(MPathElement, HgAugmentedManifestMetadata)>,
    /// Whether this directory itself is a restriction root (has a `.slacl` file).
    /// Waypoints (ancestors of restriction roots) are NOT restricted.
    pub is_restricted: bool,
    /// For each child directory in the sparse ACL tree, whether the child is
    /// itself a restriction root. Used by the SLAPI handler to populate
    /// `has_acl` on child `TreeChildEntry::Directory` entries.
    pub child_restriction_map: HashMap<MPathElement, bool>,
}

// Each manifest revision contains a list of the file revisions in each changeset, in the form:
//
// <filename>\0<hex file revision id>[<flags>]\n
//
// Source: mercurial/parsers.c:parse_manifest()

pub fn serialize_manifest(
    sharded_augmented_manifest: &[(MPathElement, HgAugmentedManifestMetadata)],
) -> Result<Bytes> {
    let mut contents = Vec::new();
    for (name, subentry) in sharded_augmented_manifest {
        contents.extend(name.as_ref());
        let (tag, hash) = match subentry {
            HgAugmentedManifestEntry::DirectoryNode(directory) => {
                (HgManifestType::Tree.manifest_suffix()?, directory.treenode)
            }
            HgAugmentedManifestEntry::FileNode(file) => {
                let tag = HgManifestType::File(file.file_type).manifest_suffix()?;
                (tag, file.filenode)
            }
        };
        contents.push(b'\0');
        contents.extend(hash.to_hex().as_bytes());
        contents.extend(tag);
        contents.push(b'\n')
    }
    Ok(contents.into())
}

impl HgPreloadedAugmentedManifest {
    pub async fn load_from_sharded<'a>(
        sharded_manifest: HgAugmentedManifestEnvelope,
        ctx: &'a CoreContext,
        blobstore: &'a impl KeyedBlobstore,
    ) -> Result<Self> {
        let augmented_manifest_id = sharded_manifest.augmented_manifest_id;
        let augmented_manifest_size = sharded_manifest.augmented_manifest_size;
        let hg_node_id = sharded_manifest.augmented_manifest.hg_node_id;
        let p1 = sharded_manifest.augmented_manifest.p1;
        let p2 = sharded_manifest.augmented_manifest.p2;

        let acl_manifest_directory_id = sharded_manifest
            .augmented_manifest
            .acl_manifest_directory_id;

        // Load children and ACL restriction info in parallel
        let children_fut = sharded_manifest
            .augmented_manifest
            .into_subentries(ctx, blobstore)
            .try_collect::<Vec<_>>();

        let acl_info_fut = async {
            match acl_manifest_directory_id {
                Some(acl_id) => {
                    let acl_manifest: AclManifest =
                        blobstore::Loadable::load(&acl_id, ctx, blobstore).await?;
                    let is_restricted = matches!(
                        acl_manifest.restriction,
                        AclManifestDirectoryRestriction::Restricted(_)
                    );
                    let child_map: HashMap<MPathElement, bool> = acl_manifest
                        .into_subentries(ctx, blobstore)
                        .try_filter_map(|(name, entry)| async move {
                            match entry {
                                AclManifestEntry::Directory(dir) => {
                                    Ok(Some((name, dir.is_restricted)))
                                }
                                AclManifestEntry::AclFile(_) => Ok(None),
                            }
                        })
                        .try_collect()
                        .await?;
                    anyhow::Ok((is_restricted, child_map))
                }
                None => Ok((false, HashMap::new())),
            }
        };

        let (children_augmented_metadata, (is_restricted, child_restriction_map)) =
            future::try_join(children_fut, acl_info_fut).await?;

        let contents = serialize_manifest(&children_augmented_metadata)?;

        Ok(Self {
            hg_node_id,
            p1,
            p2,
            augmented_manifest_id,
            augmented_manifest_size,
            children_augmented_metadata,
            contents,
            is_restricted,
            child_restriction_map,
        })
    }
}
