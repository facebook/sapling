/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
use futures::stream::BoxStream;
pub use types::CasDigest;
pub use types::CasDigestType;
pub use types::CasFetchedStats;

pub fn new(config: Arc<dyn Config>) -> anyhow::Result<Option<Arc<dyn CasClient>>> {
    match factory::call_constructor::<_, Arc<dyn CasClient>>(&config as &dyn Config) {
        Ok(client) => {
            tracing::debug!(target: "cas", "created client");
            Ok(Some(client))
        }
        Err(err) => {
            if factory::is_error_from_constructor(&err) {
                tracing::debug!(target: "cas", ?err, "error creating client");
                Err(err)
            } else {
                tracing::debug!(target: "cas", "no constructors produced a client");
                Ok(None)
            }
        }
    }
}

#[async_trait::async_trait]
#[auto_impl::auto_impl(&, Box, Arc)]
pub trait CasClient: Sync + Send {
    /// Fetch blobs from CAS.
    async fn fetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<
        'a,
        anyhow::Result<(
            CasFetchedStats,
            Vec<(CasDigest, anyhow::Result<Option<Vec<u8>>>)>,
        )>,
    >;

    /// Prefetch blobs into the CAS cache
    /// Returns a stream of (stats, digests_prefetched, digests_not_found) tuples.
    async fn prefetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<'a, anyhow::Result<(CasFetchedStats, Vec<CasDigest>, Vec<CasDigest>)>>;
}
