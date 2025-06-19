/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use remote_execution_common::TDigest;
use remote_execution_common::THashAlgo;
use remote_execution_common::TLocalCacheStats;
use remote_execution_common::TStorageBackendStats;
use remote_execution_common::TStorageBackendType;
use types::Blake3;
use types::CasDigest;
use types::CasFetchedStats;

pub fn to_re_digest(d: &CasDigest) -> remote_execution_common::TDigest {
    TDigest {
        hash: d.hash.to_hex(),
        size_in_bytes: d.size as i64,
        hash_algo: Some(THashAlgo::KEYED_BLAKE3),
        ..Default::default()
    }
}

pub fn from_re_digest(d: &TDigest) -> Result<CasDigest> {
    Ok(CasDigest {
        hash: Blake3::from_hex(d.hash.as_bytes())?,
        size: d.size_in_bytes as u64,
    })
}

pub fn parse_stats(
    stats_entries: impl Iterator<Item = (TStorageBackendType, TStorageBackendStats)>,
    local_cache_stats: TLocalCacheStats,
) -> CasFetchedStats {
    let mut stats = CasFetchedStats::default();
    for (backend, dstats) in stats_entries {
        match backend {
            TStorageBackendType::ZDB => {
                stats.total_bytes_zdb += dstats.bytes as u64;
                stats.queries_zdb += dstats.queries_count as u64
            }
            TStorageBackendType::ZGATEWAY => {
                stats.total_bytes_zgw += dstats.bytes as u64;
                stats.queries_zgw += dstats.queries_count as u64
            }
            TStorageBackendType::MANIFOLD => {
                stats.total_bytes_manifold += dstats.bytes as u64;
                stats.queries_manifold += dstats.queries_count as u64
            }
            TStorageBackendType::HEDWIG => {
                stats.total_bytes_hedwig += dstats.bytes as u64;
                stats.queries_hedwig += dstats.queries_count as u64
            }
            _ => {}
        }
    }
    stats.hits_files_local_cache = local_cache_stats.hits_files as u64;
    stats.hits_bytes_local_cache = local_cache_stats.hits_bytes as u64;
    stats.misses_files_local_cache = local_cache_stats.misses_files as u64;
    stats.misses_bytes_local_cache = local_cache_stats.misses_bytes as u64;
    stats.hits_blobs_local_lmdb_cache = local_cache_stats.hits_count_lmdb as u64;
    stats.hits_bytes_local_lmdb_cache = local_cache_stats.hits_bytes_lmdb as u64;
    stats.misses_blobs_local_lmdb_cache = local_cache_stats.misses_count_lmdb as u64;
    stats.misses_bytes_local_lmdb_cache = local_cache_stats.misses_bytes_lmdb as u64;
    stats.cloom_false_positives = local_cache_stats.cloom_false_positives as u64;
    stats.cloom_true_positives = local_cache_stats.cloom_true_positives as u64;
    stats.cloom_misses = local_cache_stats.cloom_miss_count as u64;
    stats
}

pub fn split_up_to_max_bytes(
    digests: &[CasDigest],
    max_bytes: u64,
) -> impl Iterator<Item = &[CasDigest]> {
    struct Iter<'a> {
        pub digests: &'a [CasDigest],
        pub max_bytes: u64,
    }

    impl<'a> Iterator for Iter<'a> {
        type Item = &'a [CasDigest];

        fn next(&mut self) -> Option<&'a [CasDigest]> {
            if self.digests.is_empty() {
                return None;
            }
            let mut split_at = 0;
            let mut sum = 0;
            for (i, digest) in self.digests.iter().enumerate() {
                sum += digest.size;
                if sum > self.max_bytes {
                    break;
                }
                split_at = i + 1;
            }
            if split_at == 0 {
                // We didn't find a split point meaning that there is a single file above the threshold,
                // so just return this first item.
                split_at = 1;
            }
            let (batch, rest) = self.digests.split_at(split_at);
            self.digests = rest;
            Some(batch)
        }
    }
    Iter { digests, max_bytes }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn split_and_format_results(digests: &[CasDigest], max_bytes: u64) -> String {
        split_up_to_max_bytes(digests, max_bytes)
            .map(|v| {
                v.iter()
                    .map(|d| d.size.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn test_split_up_to_max_bytes() {
        let hash =
            Blake3::from_str("2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a")
                .unwrap();
        let digests = vec![
            CasDigest { hash, size: 200 },
            CasDigest { hash, size: 200 },
            CasDigest { hash, size: 400 },
        ];

        assert_eq!(split_and_format_results(&digests, 200), "200|200|400");
        assert_eq!(split_and_format_results(&digests, 400), "200,200|400");
        assert_eq!(split_and_format_results(&digests, 500), "200,200|400");
        assert_eq!(split_and_format_results(&digests, 5000), "200,200,400");
        assert_eq!(split_and_format_results(&digests, 10), "200|200|400");
    }
}
