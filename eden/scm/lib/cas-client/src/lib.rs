/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
pub use types::CasDigest;

pub fn new(config: Arc<dyn Config>) -> anyhow::Result<Option<Arc<dyn CasClient>>> {
    match factory::call_constructor::<_, Arc<dyn CasClient>>(&config as &dyn Config) {
        Ok(client) => Ok(Some(client)),
        Err(err) => {
            if factory::is_error_from_constructor(&err) {
                Err(err)
            } else {
                Ok(None)
            }
        }
    }
}

#[async_trait::async_trait]
#[auto_impl::auto_impl(&, Box, Arc)]
pub trait CasClient: Sync + Send {
    /// Fetch blobs from CAS.
    async fn fetch(
        &self,
        digests: &[CasDigest],
    ) -> anyhow::Result<Vec<(CasDigest, anyhow::Result<Option<Vec<u8>>>)>>;
}
