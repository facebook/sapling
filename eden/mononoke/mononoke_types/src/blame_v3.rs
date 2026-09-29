/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! BlameV3: blame data keyed by HistoryManifestFileId instead of FileUnodeId.
//!
//! The blame data format is identical to BlameV2 — only the ID type and
//! blobstore key prefix change. This allows blame to be derived from
//! history manifests without depending on unodes.

use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures_watchdog::WatchdogExt;

use crate::blame_v2::BlameV2;
use crate::typed_hash::BlobstoreKey;
use crate::typed_hash::HistoryManifestFileId;
use crate::typed_hash::MononokeId;

/// Version of the BlameV3 derivation. It participates in both the blobstore
/// key and the XDB mapping row, so bumping it makes previously derived blame
/// unreachable and forces rederivation rather than reusing stored values.
pub const BLAME_V3_VERSION: i32 = 2;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BlameV3Id(HistoryManifestFileId);

impl BlameV3Id {
    pub fn blobstore_key(&self) -> String {
        format!("blame_v3.v{}.{}", BLAME_V3_VERSION, self.0.blobstore_key())
    }
    pub fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

impl From<HistoryManifestFileId> for BlameV3Id {
    fn from(hm_file_id: HistoryManifestFileId) -> Self {
        BlameV3Id(hm_file_id)
    }
}

impl From<BlameV3Id> for HistoryManifestFileId {
    fn from(blame_id: BlameV3Id) -> Self {
        blame_id.0
    }
}

impl AsRef<HistoryManifestFileId> for BlameV3Id {
    fn as_ref(&self) -> &HistoryManifestFileId {
        &self.0
    }
}

#[async_trait]
impl Loadable for BlameV3Id {
    type Value = BlameV2;

    async fn load<'a, B: KeyedBlobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let blobstore_key = self.blobstore_key();
        let bytes = blobstore
            .get(ctx, &blobstore_key)
            .watched()
            .with_max_poll(blobstore::BLOBSTORE_MAX_POLL_TIME_MS)
            .await?
            .ok_or(LoadableError::Missing(blobstore_key))?;
        let blame_t = compact_protocol::deserialize(bytes.as_raw_bytes().as_ref())?;
        let blame = BlameV2::from_thrift(blame_t)?;
        Ok(blame)
    }
}

/// Store blame object associated with a HistoryManifestFileId.
pub async fn store_blame_v3<'a, B: KeyedBlobstore>(
    ctx: &'a CoreContext,
    blobstore: &'a B,
    hm_file_id: HistoryManifestFileId,
    blame: BlameV2,
) -> Result<BlameV3Id> {
    let blame_t = blame.into_thrift();
    let data = compact_protocol::serialize(&blame_t);
    let data = BlobstoreBytes::from_bytes(data);
    let blame_id = BlameV3Id::from(hm_file_id);
    blobstore.put(ctx, blame_id.blobstore_key(), data).await?;
    Ok(blame_id)
}
