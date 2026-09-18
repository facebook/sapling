/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use blob::Blob;
use configmodel::Config;
use futures::stream::BoxStream;
pub use types::CasDigest;
pub use types::CasDigestType;

mod manager;

pub use manager::CasFetchGuard;
pub use manager::CasFetchManager;
pub use manager::CasFetchManagerBuilder;
pub use manager::CasFetchOutcome;

/// Creates the registered CAS client, if one is available in this process.
pub fn new(config: Arc<dyn Config>) -> Result<Option<Arc<CasFetchManager>>> {
    match factory::call_constructor::<_, Arc<dyn CasClient>>(&config as &dyn Config) {
        Ok(client) => Ok(Some(Arc::new(CasFetchManager::from_config(
            client,
            config.as_ref(),
        )?))),
        Err(error) if factory::is_error_from_constructor(&error) => Err(error),
        Err(_) => Ok(None),
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct CasBackendStats {
    pub total_bytes_zdb: u64,
    pub total_bytes_zgw: u64,
    pub total_bytes_manifold: u64,
    pub total_bytes_hedwig: u64,
    pub queries_zdb: u64,
    pub queries_zgw: u64,
    pub queries_manifold: u64,
    pub queries_hedwig: u64,
}

/// One result batch returned by [`CasClient::fetch`].
pub struct CasBatch {
    pub backend_stats: CasBackendStats,
    pub results: Vec<(CasDigest, Result<Option<Blob>>)>,
}

/// Fetches content-addressed blobs in batches.
pub trait CasClient: Send + Sync {
    /// Performs synchronous initialization required before fetching.
    ///
    /// Callers should invoke this from a blocking context before the first
    /// [`fetch`](Self::fetch). Implementations requiring no setup may use the
    /// default no-op.
    fn init(&self) -> Result<()> {
        Ok(())
    }

    /// Fetches `digests` as a stream of result batches.
    ///
    /// Implementations may split the input into multiple batches and yield
    /// results out of input order, so callers must associate results using the
    /// digest in each entry. Successful blobs must match the size declared by
    /// their digest.
    fn fetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        digest_type: CasDigestType,
    ) -> BoxStream<'a, Result<CasBatch>>;
}

/// Splits `digests` into contiguous batches bounded by `max_bytes` when possible.
///
/// The returned slices preserve input order and never split a digest. Empty
/// input produces no batches. A digest larger than `max_bytes` is returned
/// intact as a one-element batch, so that batch exceeds the requested bound.
pub fn split_up_to_max_bytes(digests: &[CasDigest], max_bytes: u64) -> Vec<&[CasDigest]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut bytes = 0u64;

    for (index, digest) in digests.iter().enumerate() {
        if index > start && bytes.saturating_add(digest.size) > max_bytes {
            batches.push(&digests[start..index]);
            start = index;
            bytes = 0;
        }
        bytes = bytes.saturating_add(digest.size);
    }

    if start < digests.len() {
        batches.push(&digests[start..]);
    }
    batches
}

#[cfg(test)]
mod tests {
    use types::Blake3;

    use super::*;

    fn digest(size: u64) -> CasDigest {
        CasDigest {
            hash: Blake3::from([0; 32]),
            size,
        }
    }

    #[test]
    fn splits_batches_by_total_bytes_without_splitting_a_digest() {
        let digests = [digest(200), digest(200), digest(400)];
        let batches = split_up_to_max_bytes(&digests, 400);

        assert_eq!(
            batches.iter().map(|batch| batch.len()).collect::<Vec<_>>(),
            vec![2, 1],
            "digests should stay together until adding one would exceed the byte limit"
        );
        assert_eq!(
            split_up_to_max_bytes(&digests, 10)
                .iter()
                .map(|batch| batch.len())
                .collect::<Vec<_>>(),
            vec![1, 1, 1],
            "digests larger than the limit should each remain intact"
        );
    }

    #[test]
    fn returns_no_batches_for_empty_input() {
        assert!(
            split_up_to_max_bytes(&[], 400).is_empty(),
            "empty input should not produce an empty batch"
        );
    }

    #[test]
    fn keeps_a_single_digest_in_one_batch() {
        let digests = [digest(200)];

        assert_eq!(
            split_up_to_max_bytes(&digests, 400),
            vec![digests.as_slice()],
            "a single digest within the limit should produce one unchanged batch"
        );
    }

    #[test]
    fn keeps_an_oversized_digest_in_one_batch() {
        let digests = [digest(401)];

        assert_eq!(
            split_up_to_max_bytes(&digests, 400),
            vec![digests.as_slice()],
            "a digest larger than the limit must be emitted intact rather than dropped or split"
        );
    }
}
