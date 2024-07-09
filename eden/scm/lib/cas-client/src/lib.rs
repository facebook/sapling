/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use types::CasDigest;

#[async_trait::async_trait]
pub trait CasClient: Sync + Send {
    /// Fetch blobs from CAS.
    async fn fetch(
        &self,
        digests: &[CasDigest],
    ) -> anyhow::Result<Vec<(CasDigest, anyhow::Result<Vec<u8>>)>>;
}
