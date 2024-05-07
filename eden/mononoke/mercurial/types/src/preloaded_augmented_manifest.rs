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
use mononoke_types::MPathElement;

use crate::HgAugmentedManifestEntry;
/// This is temporary type to preload Augmented Manifest and build manifest blobs in sapling native format
/// The type will be used to convert an HgAugmentedManifest entry into an EdenApi TreeEntry.
use crate::HgNodeHash;
use crate::ShardedHgAugmentedManifest;

pub type HgAugmentedManifestMetadata = HgAugmentedManifestEntry;

/// A mutable representation of a Mercurial manifest node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HgPreloadedAugmentedManifest {
    pub hg_node_id: HgNodeHash,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub contents: Bytes,
    pub children_augmented_metadata: Vec<HgAugmentedManifestMetadata>,
}

// Each manifest revision contains a list of the file revisions in each changeset, in the form:
//
// <filename>\0<hex file revision id>[<flags>]\n
//
// Source: mercurial/parsers.c:parse_manifest()

fn serrialize_manifest(
    _sharded_augmented_manifest: &[(MPathElement, HgAugmentedManifestMetadata)],
) -> Result<Bytes> {
    // The serrializion logic in unimplemented
    Ok(Bytes::new())
}

impl HgPreloadedAugmentedManifest {
    pub async fn load_from_sharded<'a>(
        sharded_manifest: ShardedHgAugmentedManifest,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> Result<Self> {
        let hg_node_id = sharded_manifest.hg_node_id;
        let p1 = sharded_manifest.p1;
        let p2 = sharded_manifest.p2;

        let data: Vec<(MPathElement, HgAugmentedManifestEntry)> = sharded_manifest
            .into_subentries(ctx, blobstore)
            .try_collect()
            .await?;

        Ok(Self {
            hg_node_id,
            p1,
            p2,
            children_augmented_metadata: data.iter().map(|(_, entry)| entry.clone()).collect(),
            contents: serrialize_manifest(&data)?,
        })
    }
}
