/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::OnceLock;

use configmodel::Config;
pub use types::CasDigest;

type Constructor = fn(&dyn Config) -> anyhow::Result<Arc<dyn CasClient>>;

static CONSTRUCTOR: OnceLock<Constructor> = OnceLock::new();

pub fn register_constructor(c: Constructor) {
    // panic if called twice
    CONSTRUCTOR.set(c).unwrap()
}

pub fn new(config: &dyn Config) -> anyhow::Result<Option<Arc<dyn CasClient>>> {
    CONSTRUCTOR.get().map(|c| c(config)).transpose()
}

#[async_trait::async_trait]
pub trait CasClient: Sync + Send {
    /// Fetch blobs from CAS.
    async fn fetch(
        &self,
        digests: &[CasDigest],
    ) -> anyhow::Result<Vec<(CasDigest, anyhow::Result<Vec<u8>>)>>;
}
