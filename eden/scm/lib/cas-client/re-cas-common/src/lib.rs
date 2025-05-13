/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use anyhow::Result;
pub use anyhow::anyhow;
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
pub use types::FetchContext;

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
        use re_client_lib::UploadRequest;
        use re_client_lib::TDigest;
        use re_client_lib::THashAlgo;
        use re_client_lib::TStorageBackendType;
        use re_client_lib::TStorageBackendStats;
        use re_client_lib::TLocalCacheStats;

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

        fn parse_stats(stats_entries: impl Iterator<Item=(TStorageBackendType, TStorageBackendStats)>, local_cache_stats: TLocalCacheStats) -> $crate::CasFetchedStats {
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

        #[$crate::async_trait]
        impl $crate::CasClient for $struct {
            /// Fetch a single blob from local CAS caches.
            fn fetch_single_locally_cached(
                &self,
                digest: &$crate::CasDigest,
            ) -> Result<($crate::CasFetchedStats, Option<Blob>)> {

                $crate::tracing::trace!(target: "cas", concat!(stringify!($struct), " fetching {:?} digest from local cache"), digest);

                #[cfg(target_os = "linux")]{
                    let (stats, data) = self.client()?
                        .low_level_lookup_cache(self.metadata.clone(), to_re_digest(digest))?.unpack();

                    let parsed_stats = parse_stats(std::iter::empty(), stats);

                    if data.is_null() {
                        return Ok((parsed_stats, None));
                    } else {
                        return Ok((parsed_stats, Some(Blob::IOBuf(data.into()))));
                    }
                }

                #[cfg(not(target_os = "linux"))]
                return Ok(($crate::CasFetchedStats::default(), None));
            }


            /// Upload blobs to CAS.
            async fn upload(
                &self,
                blobs: Vec<Blob>,
            ) -> Result<Vec<$crate::CasDigest>> {

                $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " uploading {} blobs"), blobs.len());

                #[cfg(target_os = "linux")] {
                    self.client()?
                        .co_upload_inlined_blobs(
                            self.metadata.clone(),
                            blobs.into_iter().map(|blob| {
                                blob.into_vec()
                            }).collect()
                        )
                        .await??
                        .digests
                        .into_iter()
                        .map(|digest_with_status| from_re_digest(&digest_with_status.digest))
                        .collect::<$crate::Result<Vec<_>>>()
                }

                #[cfg(not(target_os = "linux"))] {
                    self.client()?
                    .upload(
                        self.metadata.clone(),
                        UploadRequest {
                            inlined_blobs: Some(blobs.into_iter().map(|blob| {
                                    blob.into_vec()
                            }).collect()),
                            upload_only_missing: true,
                            ..Default::default()
                        },
                    )
                    .await?
                    .inlined_blobs_status
                    .unwrap_or_default()
                    .into_iter()
                    .map(|digest_with_status| from_re_digest(&digest_with_status.digest))
                    .collect::<$crate::Result<Vec<_>>>()
                }
            }


            /// Fetch blobs from CAS.
            async fn fetch<'a>(
                &'a self,
                _fctx: $crate::FetchContext,
                digests: &'a [$crate::CasDigest],
                log_name: $crate::CasDigestType,
            ) -> BoxStream<'a, $crate::Result<($crate::CasFetchedStats, Vec<($crate::CasDigest, Result<Option<Blob>>)>)>>
            {
                stream::iter(split_up_to_max_bytes(digests, self.fetch_limit.value()))
                    .map(move |digests| async move {
                        if !self.cas_success_tracker.allow_request()? {
                            $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " skip fetching {} {}(s)"), digests.len(), log_name);
                            return Err($crate::anyhow!("skip cas fetching due to cas success tracker error rate limiting vioaltion"));
                        }
                        if self.use_streaming_dowloads && digests.len() == 1 && digests.first().unwrap().size >= self.fetch_limit.value() {
                            // Single large file, fetch it via the streaming API to avoid memory issues on CAS side.
                            let digest = digests.first().unwrap();
                            $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " streaming {} {}(s)"), digests.len(), log_name);


                            // Unfortunately, the streaming API does not return the storage stats, so it won't be added to the stats.
                            let stats = $crate::CasFetchedStats::default();

                            #[cfg(target_os = "linux")]
                            let mut response_stream = self.client()?
                                .download_stream(self.metadata.clone(), to_re_digest(digest))
                                .await;

                            #[cfg(not(target_os = "linux"))]
                            let mut response_stream = {
                                let request =  DownloadStreamRequest {
                                    digest: to_re_digest(digest),
                                    ..Default::default()
                                };
                                let response = self.client()?
                                    .download_stream(self.metadata.clone(), request)
                                    .await;

                                if let Err(ref err) = response {
                                    if let Some(inner) = err.downcast_ref::<REClientError>() {
                                        if inner.code == TCode::NOT_FOUND {
                                            // Streaming download failed because the digest was not found, record a success.
                                            self.cas_success_tracker.record_success();
                                            return Ok((stats, vec![(digest.to_owned(), Ok(None))]));
                                        }
                                        // Unfortunately, the streaming download failed, record a failure.
                                        self.cas_success_tracker.record_failure()?;
                                    }
                                }
                                response
                            }?;

                            let mut bytes: Vec<u8> = Vec::with_capacity(digest.size as usize);
                            while let Some(chunk) = response_stream.next().await {
                                if let Err(ref _err) = chunk {
                                    self.cas_success_tracker.record_failure()?;
                                }
                                bytes.extend(chunk?.data);
                            }

                            self.cas_success_tracker.record_success();
                            return Ok((stats, vec![(digest.to_owned(), Ok(Some(Blob::Bytes(bytes.into()))))]));
                        }

                        // Fetch digests via the regular API (download inlined digests).

                        $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " fetching {} {}(s)"), digests.len(), log_name);

                        #[cfg(target_os = "linux")]
                        let (data, stats) = {
                            let response = self.client()?
                                .co_low_level_download_inline(self.metadata.clone(), digests.iter().map(to_re_digest).collect()).await;

                            if let Err(ref err) = response {
                                if (err.code == TCode::NOT_FOUND) {
                                    $crate::tracing::warn!(target: "cas", "digest not found and can not be fetched: {:?}", digests);
                                }
                            }

                            let response = response.map_err(|err| {
                                    // Unfortunately, the download failed entirely, record a failure.
                                    let _failure_error = self.cas_success_tracker.record_failure();
                                    err
                                })?;

                            let mut local_cache_stats = response.get_local_cache_stats();
                            let mut storage_stats = response.get_storage_stats();
                            (response.unpack_downloads(),  parse_stats(storage_stats.per_backend_stats.into_iter(), local_cache_stats))
                        };

                        #[cfg(not(target_os = "linux"))]
                        let (data, stats) = {
                            let request = DownloadRequest {
                                inlined_digests: Some(digests.iter().map(to_re_digest).collect()),
                                throw_on_error: false,
                                ..Default::default()
                            };
                            let response = self.client()?
                            .download(self.metadata.clone(), request)
                            .await
                            .map_err(|err| {
                                // Unfortunately, the download failed entirely, record a failure.
                                let _failure_error = self.cas_success_tracker.record_failure();
                                err
                            })?;
                            let local_cache_stats = response.local_cache_stats;

                            (response.inlined_blobs.unwrap_or_default(), parse_stats(response.storage_stats.per_backend_stats.into_iter(), local_cache_stats))
                        };


                        let data = data
                            .into_iter()
                            .map(|blob| {
                                #[cfg(target_os = "linux")]
                                let (digest, status, data) = {
                                    let (digest, status, data) = blob.unpack();
                                    (digest, status, Blob::IOBuf(data.into()))
                                };
                                #[cfg(not(target_os = "linux"))]
                                let (digest, status, data) = (blob.digest, blob.status, Blob::Bytes(blob.blob.into()));

                                let digest = from_re_digest(&digest)?;
                                match status.code {
                                    TCode::OK => Ok((digest, Ok(Some(data)))),
                                    TCode::NOT_FOUND => Ok((digest, Ok(None))),
                                    _ => Ok((
                                        digest,
                                        Err($crate::anyhow!(
                                            "bad status (code={}, message={}, group={})",
                                            status.code,
                                            status.message,
                                            status.group
                                        )),
                                    )),
                                }
                            })
                            .collect::<$crate::Result<Vec<_>>>()?;

                        // If all digests are failed, report a failure.
                        // Otherwise, report a success (could be a partial success)
                        let all_errors = data.iter().all(|(_, result)| result.is_err());
                        if all_errors {
                            self.cas_success_tracker.record_failure()?;
                        } else {
                            self.cas_success_tracker.record_success();
                        }

                        Ok((stats, data))
                    })
                    .buffer_unordered(self.fetch_concurrency)
                    .boxed()
            }

        /// Prefetch blobs into the CAS local caches.
        /// Returns a stream of (stats, digests_prefetched, digests_not_found) tuples.
        async fn prefetch<'a>(
            &'a self,
            _fctx: $crate::FetchContext,
            digests: &'a [$crate::CasDigest],
            log_name: $crate::CasDigestType,
        ) -> BoxStream<'a, $crate::Result<($crate::CasFetchedStats, Vec<$crate::CasDigest>, Vec<$crate::CasDigest>)>>
        {
            stream::iter(split_up_to_max_bytes(digests, self.fetch_limit.value()))
                .map(move |digests| async move {
                    if !self.cas_success_tracker.allow_request()? {
                        $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " skip prefetching {} {}(s)"), digests.len(), log_name);
                        return Err($crate::anyhow!("skip cas prefetching due to cas success tracker error rate limiting vioaltion"));
                    }

                    $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " prefetching {} {}(s)"), digests.len(), log_name);

                    #[cfg(target_os = "linux")]
                    let response = self.client()?
                        .co_download_digests_into_cache(self.metadata.clone(), digests.into_iter().map(to_re_digest).collect())
                        .await
                        .map_err(|err| {
                            // Unfortunately, the "download_digests_into_cache" failed entirely, record a failure.
                            let _failure_error = self.cas_success_tracker.record_failure();
                            err
                        })?;

                    #[cfg(not(target_os = "linux"))]
                    let response = {
                        let request = DownloadDigestsIntoCacheRequest {
                            digests: digests.iter().map(to_re_digest).collect(),
                            throw_on_error: false,
                            ..Default::default()
                        };
                        self.client()?
                        .download_digests_into_cache(self.metadata.clone(), request)
                        .await
                        .map_err(|err| {
                            // Unfortunately, the "download_digests_into_cache" failed entirely, record a failure.
                            let _failure_error = self.cas_success_tracker.record_failure();
                            err
                        })
                    }?;

                    let local_cache_stats = response.local_cache_stats;

                    let stats = parse_stats(response.storage_stats.per_backend_stats.into_iter(), local_cache_stats);

                    let data = response.digests_with_status
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
                        .collect::<Result<Vec<_>>>();

                    // If all digests are failed, report a failure.
                    // Otherwise, report a success (could be a partial success)
                    if let Err(_) = data {
                        self.cas_success_tracker.record_failure()?;
                    } else {
                        self.cas_success_tracker.record_success();
                    }

                    let (digests_prefetched, digests_not_found) = data?.into_iter()
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
