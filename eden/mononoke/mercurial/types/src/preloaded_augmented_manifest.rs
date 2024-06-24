/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Blobstore;
use bytes::Bytes;
use context::CoreContext;
use futures::TryStreamExt;
use mononoke_types::hash::Blake3;
use mononoke_types::MPathElement;

use crate::manifest::Type as HgManifestType;
use crate::HgAugmentedManifestEntry;
use crate::HgAugmentedManifestEnvelope;
/// This is temporary type to preload Augmented Manifest and build manifest blobs in sapling native format
/// The type will be used to convert an HgAugmentedManifest entry into an SaplingRemoteApi TreeEntry.
use crate::HgNodeHash;

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
}

// Each manifest revision contains a list of the file revisions in each changeset, in the form:
//
// <filename>\0<hex file revision id>[<flags>]\n
//
// Source: mercurial/parsers.c:parse_manifest()

fn serrialize_manifest(
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
        blobstore: &'a impl Blobstore,
    ) -> Result<Self> {
        let augmented_manifest_id = sharded_manifest.augmented_manifest_id;
        let augmented_manifest_size = sharded_manifest.augmented_manifest_size;
        let hg_node_id = sharded_manifest.augmented_manifest.hg_node_id;
        let p1 = sharded_manifest.augmented_manifest.p1;
        let p2 = sharded_manifest.augmented_manifest.p2;

        let children_augmented_metadata: Vec<(MPathElement, HgAugmentedManifestEntry)> =
            sharded_manifest
                .augmented_manifest
                .into_subentries(ctx, blobstore)
                .try_collect()
                .await?;

        let contents = serrialize_manifest(&children_augmented_metadata)?;

        Ok(Self {
            hg_node_id,
            p1,
            p2,
            augmented_manifest_id,
            augmented_manifest_size,
            children_augmented_metadata,
            contents,
        })
    }
}
