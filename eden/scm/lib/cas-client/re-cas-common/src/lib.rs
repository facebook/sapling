/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use anyhow::anyhow;
pub use anyhow::Result;
pub use async_trait::async_trait;
pub use cas_client::CasClient;
pub use itertools::Either;
pub use itertools::Itertools;
pub use once_cell::sync::OnceCell;
pub use tracing;
pub use types::Blake3;
pub use types::CasDigest;
pub use types::CasDigestType;
pub use types::CasFetchedStats;
pub use types::CasPrefetchOutcome;

#[macro_export]
macro_rules! re_client {
    ( $struct:tt ) => {
        use futures::stream;
        use futures::stream::BoxStream;
        use futures::StreamExt;
        use futures::TryStreamExt;
        use re_cas_common::split_up_to_max_bytes;
        use re_cas_common::Itertools;
        use re_client_lib::DownloadRequest;
        use re_client_lib::DownloadDigestsIntoCacheRequest;
        use re_client_lib::DownloadStreamRequest;
        use re_client_lib::REClientError;
        use re_client_lib::TCode;
        use re_client_lib::TDigest;
        use re_client_lib::THashAlgo;
        use re_client_lib::TStorageBackendType;
        use re_client_lib::TStorageBackendStats;

        impl $struct {
            fn client(&self) -> Result<&REClient> {
                self.client.get_or_try_init(|| self.build())
            }
        }

        fn to_re_digest(d: &$crate::CasDigest) -> TDigest {
            TDigest {
                hash: d.hash.to_hex(),
                size_in_bytes: d.size as i64,
                hash_algo: Some(THashAlgo::BLAKE3),
                ..Default::default()
            }
        }

        fn from_re_digest(d: &TDigest) -> $crate::Result<$crate::CasDigest> {
            Ok($crate::CasDigest {
                hash: $crate::Blake3::from_hex(d.hash.as_bytes())?,
                size: d.size_in_bytes as u64,
            })
        }

        fn parse_stats(stats_entries: impl Iterator<Item=(TStorageBackendType, TStorageBackendStats)>) -> $crate::CasFetchedStats {
            let mut stats = $crate::CasFetchedStats::default();
            for (backend, dstats) in stats_entries {
                match backend {
                        TStorageBackendType::ZDB => {stats.total_bytes_zdb += dstats.bytes as u64; stats.queries_zdb += dstats.queries_count as u64}
                        TStorageBackendType::ZGATEWAY => {stats.total_bytes_zgw += dstats.bytes as u64; stats.queries_zgw += dstats.queries_count as u64}
                        TStorageBackendType::MANIFOLD => {stats.total_bytes_manifold += dstats.bytes as u64; stats.queries_manifold += dstats.queries_count as u64}
                        TStorageBackendType::HEDWIG => {stats.total_bytes_hedwig += dstats.bytes as u64; stats.queries_hedwig += dstats.queries_count as u64 }
                        _ => {}
                }
            }
            stats
        }

        #[$crate::async_trait]
        impl $crate::CasClient for $struct {
            async fn fetch<'a>(
                &'a self,
                digests: &'a [$crate::CasDigest],
                log_name: $crate::CasDigestType,
            ) -> BoxStream<'a, $crate::Result<($crate::CasFetchedStats, Vec<($crate::CasDigest, Result<Option<Vec<u8>>>)>)>>
            {
                stream::iter(split_up_to_max_bytes(digests, self.fetch_limit.value()))
                    .map(move |digests| async move {

                        if self.use_streaming_dowloads && digests.len() == 1 && digests.first().unwrap().size >= self.fetch_limit.value() {
                            // Single large file, fetch it via the streaming API to avoid memory issues on CAS side.
                            let digest = digests.first().unwrap();
                            $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " streaming {} {}(s)"), digests.len(), log_name);
                            let request =  DownloadStreamRequest {
                                digest: to_re_digest(digest),
                                ..Default::default()
                            };

                            // Unfortunately, the streaming API does not return the storage stats, so it won't be added to the stats.
                            let stats = $crate::CasFetchedStats::default();

                            let response = self.client()?
                                .download_stream(self.metadata.clone(), request)
                                .await;

                            if let Err(ref err) = response {
                                if let Some(inner) = err.downcast_ref::<REClientError>() {
                                    if inner.code == TCode::NOT_FOUND {
                                        return Ok((stats, vec![(digest.to_owned(), Ok(None))]));
                                    }
                                }
                            }

                            let mut bytes: Vec<u8> = Vec::with_capacity(digest.size as usize);
                            let mut response_stream = response?;
                            while let Some(chunk) = response_stream.next().await {
                                bytes.extend(chunk?.data);
                            }

                            return Ok((stats, vec![(digest.to_owned(), Ok(Some(bytes)))]));
                        }

                        // Fetch digests via the regular API (download inlined digests).

                        $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " fetching {} {}(s)"), digests.len(), log_name);

                        let request = DownloadRequest {
                            inlined_digests: Some(digests.iter().map(to_re_digest).collect()),
                            throw_on_error: false,
                            ..Default::default()
                        };

                        let response = self.client()?
                            .download(self.metadata.clone(), request)
                            .await?;

                        let stats = parse_stats(response.storage_stats.per_backend_stats.into_iter());

                        let data = response.inlined_blobs
                            .unwrap_or_default()
                            .into_iter()
                            .map(|blob| {
                                let digest = from_re_digest(&blob.digest)?;
                                match blob.status.code {
                                    TCode::OK => Ok((digest, Ok(Some(blob.blob)))),
                                    TCode::NOT_FOUND => Ok((digest, Ok(None))),
                                    _ => Ok((
                                        digest,
                                        Err($crate::anyhow!(
                                            "bad status (code={}, message={}, group={})",
                                            blob.status.code,
                                            blob.status.message,
                                            blob.status.group
                                        )),
                                    )),
                                }
                            })
                            .collect::<$crate::Result<Vec<_>>>()?;

                        Ok((stats, data))
                    })
                    .buffer_unordered(self.fetch_concurrency)
                    .boxed()
            }

        /// Prefetch digests into the cache.
        /// Returns a stream of (stats, digests_prefetched, digests_not_found) tuples.
        async fn prefetch<'a>(
            &'a self,
            digests: &'a [$crate::CasDigest],
            log_name: $crate::CasDigestType,
        ) -> BoxStream<'a, $crate::Result<($crate::CasFetchedStats, Vec<$crate::CasDigest>, Vec<$crate::CasDigest>)>>
        {
            stream::iter(split_up_to_max_bytes(digests, self.fetch_limit.value()))
                .map(move |digests| async move {
                    $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " prefetching {} {}(s)"), digests.len(), log_name);

                    let request = DownloadDigestsIntoCacheRequest {
                        digests: digests.iter().map(to_re_digest).collect(),
                        throw_on_error: false,
                        ..Default::default()
                    };

                    let response = self.client()?
                        .download_digests_into_cache(self.metadata.clone(), request)
                        .await;

                    // Unfortunately, the download_digests_into_cache fails entirely with NOT_FOUND if a digest is not found.
                    // For now, let's report that everything is missing instead of failing the entire prefetch.
                    // The issue should be fixed on RE side, so that they can provide correct per digest statuses.
                    if let Err(ref err) = response {
                        if let Some(inner) = err.downcast_ref::<REClientError>() {
                            if inner.code == TCode::NOT_FOUND {
                                return Ok(($crate::CasFetchedStats::default(), Vec::new(), digests.to_vec()));
                            }
                        }
                    }

                    let response = response?;

                    let stats = parse_stats(response.storage_stats.per_backend_stats.into_iter());

                    let (digests_prefetched, digests_not_found) = response.digests_with_status
                        .into_iter()
                        .map(|blob| {
                            let digest = from_re_digest(&blob.digest)?;
                            match blob.status.code {
                                TCode::OK => Ok($crate::CasPrefetchOutcome::Prefetched(digest)),
                                TCode::NOT_FOUND => {
                                    $crate::tracing::warn!(target: "cas", "digest not found and can not be prefetched: {:?}", digest);
                                    Ok($crate::CasPrefetchOutcome::Missing(digest))
                                },
                                _ => Err($crate::anyhow!(
                                        "bad status (code={}, message={}, group={})",
                                        blob.status.code,
                                        blob.status.message,
                                        blob.status.group
                                    )),
                            }
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .partition_map(|outcome| match outcome {
                            $crate::CasPrefetchOutcome::Prefetched(digest) => $crate::Either::Left(digest),
                            $crate::CasPrefetchOutcome::Missing(digest) => $crate::Either::Right(digest),
                        });

                    Ok((stats, digests_prefetched, digests_not_found))
                })
                .buffer_unordered(self.fetch_concurrency)
                .boxed()
        }
    }
    };
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
