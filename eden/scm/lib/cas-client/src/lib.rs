/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use blob::Blob;
use futures::stream::BoxStream;
pub use types::CasDigest;
pub use types::CasDigestType;

/// Per-digest results from one batch returned by [`CasClient::fetch`].
///
/// Each entry associates a digest with its blob, a CAS not-found result
/// (`Ok(None)`), or an error specific to that digest.
pub type CasBatch = Vec<(CasDigest, Result<Option<Blob>>)>;

/// Fetches content-addressed blobs in batches.
pub trait CasClient: Send + Sync {
    /// Performs optional eager initialization.
    ///
    /// Implementations that initialize lazily can use the default no-op. This
    /// method lets callers surface setup failures separately from fetches.
    fn init(&self) -> Result<()> {
        Ok(())
    }

    /// Fetches `digests` as a stream of result batches.
    ///
    /// Implementations may split the input into multiple batches and yield
    /// results out of input order, so callers must associate results using the
    /// digest in each entry.
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
