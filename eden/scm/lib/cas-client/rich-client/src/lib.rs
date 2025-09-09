/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use blob::Blob;
use cas_client::CasClient;
use cas_client::CasDigest;
use cas_client::CasDigestType;
use cas_client::CasFetchedStats;
use cas_client::CasSuccessTracker;
use cas_client::CasSuccessTrackerConfig;
use cas_client::FetchContext;
use cas_client_lib::CASDaemonClientCfg;
use cas_client_lib::EmbeddedCASDaemonClientCfg;
use cas_client_lib::RESessionID;
use cas_client_lib::RemoteCASdAddress;
use cas_client_lib::RemoteCacheConfig;
use cas_client_lib::RemoteCacheManagerMode;
use cas_client_lib::RemoteExecutionMetadata;
use cas_client_lib::RemoteFetchPolicy;
use cas_client_lib::TCode;
use cas_client_lib::THashAlgo;
use cas_client_lib::TQuotaPoolInfo;
use cas_client_lib::create_default_config;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use futures::StreamExt;
use futures::stream;
use futures::stream::BoxStream;
use itertools::Either;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use re_cas_common::from_re_digest;
use re_cas_common::parse_stats;
use re_cas_common::split_up_to_max_bytes;
use re_cas_common::to_re_digest;
use rich_cas_client_wrapper::CASClientWrapper as CASClientBundle;
use rich_cas_client_wrapper::CASSharedCacheWrapper;
use types::cas::CasPrefetchOutcome;

pub const CAS_SOCKET_PATH: &str = "/run/casd/casd.socket";
pub const CAS_SESSION_TTL: i64 = 600; // 10 minutes

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CasCacheModeLocalFetch {
    /// All files are fetched from the storages (zgw, zdb, manifold, etc) directly (rich client mode),
    AllFilesLocally,
    /// Small files are fetched from the storages (zgw, zdb, manifold, etc) directly (rich client mode), while the large files are fetched from the CASd daemon via UDS.
    SmallFilesLocally,
    /// All files are fetched from the CASd daemon via UDS.
    AllRemote,
}

pub struct RichCasClient {
    client: OnceCell<CASClientBundle>,
    shared_cache: OnceCell<CASSharedCacheWrapper>,
    cas_success_tracker: CasSuccessTracker,
    /// Verbose logging will disable quiet mode in REClient.
    verbose: bool,
    /// The log directory to store the logs of the CASd daemon.
    log_dir: Option<String>,
    /// The session id of the client. Used to trace RE session operations.
    session_id: String,
    /// Contains the use case id information.
    metadata: RemoteExecutionMetadata,
    /// Whether to use the shared cache or not (CASd daemon).
    /// Normally the WDB Casd daemon is used, but it can also be local for testing/benchmarking.
    use_casd_cache: bool,
    /// The socket path to connect to the shared cache (CASd daemon).
    /// Normally the WDB Casd daemon is used, but it can also be local for testing/benchmarking.
    casd_cache_socket_path: String,
    /// The mode to use for local fetching (aka rich client direct fetch from the backends).
    /// Could be all files, small files, or not enabled at all, so all the fetches are done via CASd daemon.
    /// The fetched blobs will be synced to the local cache asynchronously (if enabled).
    cas_cache_mode_local_fetch: Option<CasCacheModeLocalFetch>,
    /// The maximum number of bytes to fetch in a single batch combined.
    fetch_limit: ByteCount,
    /// The maximum number of concurrent batches to fetch.
    /// fetch_limit * fetch_concurrency is the maximum number of bytes to fetch in parallel, so memory usage is bounded.
    fetch_concurrency: usize,
    /// Whether to use streaming downloads or not for very large files (hundreds of MBs).
    use_streaming_downloads: bool,
    /// The path to the private cache (local cache).
    /// This mode is used for testing/benchmarking, and it is not used in production.
    /// This mode would define Rich client mode with its own local cache, the shared cache (CASd daemon) is not used.
    private_cache_path: Option<String>,
    /// The size of the private cache (local cache).
    private_cache_size: ByteCount,
    /// Whether to use persistent caches or not.
    use_persistent_caches: bool,
}

pub fn init() {
    fn construct(config: &dyn Config) -> Result<Option<Arc<dyn CasClient>>> {
        tracing::debug!(target: "cas_client", "creating rich client");
        RichCasClient::from_config(config).map(|c| c.map(|c| Arc::new(c) as Arc<dyn CasClient>))
    }
    factory::register_constructor("rich-client", construct);
}

impl RichCasClient {
    pub fn from_config(config: &dyn Config) -> Result<Option<Self>> {
        let use_case: String = match config.get("cas", "use-case") {
            Some(use_case) => use_case.to_string(),
            None => {
                let repo_name = match config
                    .get_nonempty_opt::<String>("remotefilelog", "reponame")?
                {
                    Some(repo_name) => repo_name,
                    None => {
                        tracing::info!(target: "cas_client", "no use case or repo name configured");
                        return Ok(None);
                    }
                };
                format!("source-control-{repo_name}")
            }
        };

        let use_casd_cache = config.get_or("cas", "use-shared-cache", || true)?;
        let casd_cache_socket_path =
            config.get_or("cas", "uds-path", || CAS_SOCKET_PATH.to_string())?;

        let mut cas_cache_mode_local_fetch = None;

        if use_casd_cache {
            cas_cache_mode_local_fetch = Some(CasCacheModeLocalFetch::AllRemote);

            if config.get_or("cas", "shared_cache.local.small_files", || true)? {
                cas_cache_mode_local_fetch = Some(CasCacheModeLocalFetch::SmallFilesLocally);
            }

            if config.get_or("cas", "shared_cache.local.all_files", || false)? {
                cas_cache_mode_local_fetch = Some(CasCacheModeLocalFetch::AllFilesLocally);
            }
        }

        let default_fetch_limit = ByteCount::try_from_str("200MB")?;

        let private_cache_path = config.get_opt::<String>("cas", "private-cache-path")?;
        if private_cache_path.is_some() && use_casd_cache {
            return Err(anyhow::anyhow!(
                "cas.private-cache-path and cas.use-shared-cache cannot be used together"
            ));
        }
        let default_private_cache_size = ByteCount::try_from_str("100GB")?;
        let private_cache_size = config
            .get_or::<ByteCount>("cas", "private-cache-size", || default_private_cache_size)?;

        let cri = clientinfo::get_client_request_info();
        let session_id = format!("{}_{}", cri.entry_point, cri.correlator);

        let log_dir = config
            .get_or("cas", "log-dir", || None)
            .ok()
            .flatten()
            .map(|log_dir| {
                std::fs::create_dir_all(&log_dir).unwrap_or_else(|e| {
                    panic!("Failed to create log directory {:?}: {:?}", log_dir, e);
                });
                log_dir
            });

        Ok(Some(Self {
            client: Default::default(),
            shared_cache: Default::default(),
            verbose: config.get_or_default("cas", "verbose")?,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case.clone(),
                quota_pool_info: TQuotaPoolInfo {
                    // TODO(T228252905)
                    budget_entity: "3199644040305541".to_string(),
                    quota_pool: use_case,
                    ..Default::default()
                },
                re_session_id: Some(RESessionID {
                    id: session_id.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            use_casd_cache,
            session_id,
            log_dir,
            casd_cache_socket_path,
            cas_cache_mode_local_fetch,
            fetch_limit: config
                .get_or::<ByteCount>("cas", "max-batch-bytes", || default_fetch_limit)?,
            fetch_concurrency: config.get_or("cas", "fetch-concurrency", || 4)?,
            use_streaming_downloads: config.get_or("cas", "use-streaming-downloads", || true)?,
            private_cache_path,
            private_cache_size,
            cas_success_tracker: CasSuccessTracker::new(CasSuccessTrackerConfig {
                max_failures: config.get_or("cas", "max-failures", || 10)?,
                downtime_on_failure: config
                    .get_or("cas", "downtime-on-failure", || Duration::from_secs(1))?,
            }),
            use_persistent_caches: config.get_or("cas", "use-persistent-caches", || true)?,
        }))
    }

    fn build(&self) -> Result<CASClientBundle> {
        let mut re_config = create_default_config();

        re_config.client_name = Some("sapling".to_string());
        re_config.quiet_mode = !self.verbose;
        re_config.log_file_location = self.log_dir.clone();
        re_config.features_config_path = "remote_execution/features/client_eden".to_string();

        let mut embedded_config = EmbeddedCASDaemonClientCfg {
            name: "source_control".to_string(),
            ..Default::default()
        };
        if self.use_casd_cache {
            let mut remote_cache_config = RemoteCacheConfig {
                address: RemoteCASdAddress::uds_path(self.casd_cache_socket_path.clone()),
                ..Default::default()
            };
            if let Some(cas_cache_mode_local_fetch) = self.cas_cache_mode_local_fetch {
                match cas_cache_mode_local_fetch {
                    // In EdenFS we only use the inline blobs fetches, there the sync is not yet implemented.
                    // So, "WITH_SYNC" is no op.
                    // The local fetch means that the data is fetched from the storages (zgw, zdb, manifold, etc) directly (rich client mode),
                    // while the remote fetch means that the data is fetched from the CASd daemon via UDS.
                    CasCacheModeLocalFetch::AllFilesLocally => {
                        remote_cache_config.mode = RemoteCacheManagerMode::ALL_FILES;
                        remote_cache_config.small_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC;
                        remote_cache_config.large_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC;
                    }
                    CasCacheModeLocalFetch::SmallFilesLocally => {
                        embedded_config.rich_client_config.disable_p2p = true;
                        remote_cache_config.mode = RemoteCacheManagerMode::ALL_FILES;
                        remote_cache_config.small_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC;
                        remote_cache_config.large_files = RemoteFetchPolicy::REMOTE_FETCH;
                    }
                    CasCacheModeLocalFetch::AllRemote => {
                        embedded_config.rich_client_config.disable_p2p = true;
                        remote_cache_config.mode = RemoteCacheManagerMode::ALL_FILES;
                        remote_cache_config.small_files = RemoteFetchPolicy::REMOTE_FETCH;
                        remote_cache_config.large_files = RemoteFetchPolicy::REMOTE_FETCH;
                    }
                }
            }
            embedded_config.remote_cache_config = Some(remote_cache_config);
            embedded_config.cache_config.writable_cache = false;
        }
        // We check that the modes use_casd_cache and private_cache_path do not conflict while parcing the sapling config.
        // So, if we are here, we know that the private cache is enabled.
        if let Some(ref private_cache_path) = self.private_cache_path {
            embedded_config.cache_config.downloads_cache_config.dir_path =
                Some(private_cache_path.clone());
            embedded_config
                .cache_config
                .downloads_cache_config
                .size_bytes = self.private_cache_size.value() as i64;
            embedded_config.cache_config.writable_cache = true;
        }
        embedded_config.rich_client_config.enable_rich_client = true;

        if self.use_persistent_caches {
            embedded_config.client_label = "pc_enabled".to_string();
        }

        re_config.cas_client_config = CASDaemonClientCfg::embedded_config(embedded_config);

        let client = CASClientBundle::new(self.session_id.clone(), CAS_SESSION_TTL, re_config)?;
        Ok(client)
    }

    fn client(&self) -> Result<&CASClientBundle> {
        self.client.get_or_try_init(|| self.build())
    }

    fn shared_cache_wrapper(&self) -> Result<&CASSharedCacheWrapper> {
        self.shared_cache
            .get_or_try_init(|| Ok(self.client()?.get_shared_cache_wrapper()?))
    }
}

#[async_trait::async_trait]
impl CasClient for RichCasClient {
    /// Fetch a single blob from local CAS caches.
    fn fetch_single_locally_cached(
        &self,
        digest: &CasDigest,
    ) -> Result<(CasFetchedStats, Option<Blob>)> {
        tracing::trace!(target: "cas_client", "RichCasClient fetching {:?} digest from local cache", digest);

        let (stats, data) = self
            .shared_cache_wrapper()?
            .lookup_cache(to_re_digest(digest), THashAlgo::KEYED_BLAKE3)?
            .unpack();

        let parsed_stats = parse_stats(std::iter::empty(), stats);

        if data.is_null() {
            Ok((parsed_stats, None))
        } else {
            Ok((parsed_stats, Some(Blob::IOBuf(data.into()))))
        }
    }

    /// Upload blobs to CAS.
    async fn upload(&self, blobs: Vec<Blob>) -> Result<Vec<CasDigest>> {
        tracing::debug!(target: "cas_client", "RichCasClient uploading {} blobs", blobs.len());

        self.client()?
            .co_upload_inlined_blobs(
                self.metadata.clone(),
                blobs.into_iter().map(|blob| blob.into_vec()).collect(),
            )
            .await??
            .digests
            .into_iter()
            .map(|digest_with_status| from_re_digest(&digest_with_status.digest))
            .collect::<Result<Vec<_>>>()
    }

    /// Fetch blobs from CAS.
    async fn fetch<'a>(
        &'a self,
        _fctx: FetchContext,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<'a, Result<(CasFetchedStats, Vec<(CasDigest, Result<Option<Blob>>)>)>> {
        stream::iter(split_up_to_max_bytes(digests, self.fetch_limit.value()))
            .map(move |digests| async move {
                if !self.cas_success_tracker.allow_request()? {
                    tracing::debug!(target: "cas_client", "RichCasClient skip fetching {} {}(s)", digests.len(), log_name);
                    return Err(anyhow!("skip cas fetching due to cas success tracker error rate limiting violation"));
                }
                if self.use_streaming_downloads && digests.len() == 1 && digests.first().unwrap().size >= self.fetch_limit.value() {
                    // Single large file, fetch it via the streaming API to avoid memory issues on CAS side.
                    let digest = digests.first().unwrap();
                    tracing::debug!(target: "cas_client", "RichCasClient streaming {} {}(s)", digests.len(), log_name);


                    // Unfortunately, the streaming API does not return the storage stats, so it won't be added to the stats.
                    let stats = CasFetchedStats::default();

                    let mut response_stream = self.client()?
                        .download_stream(self.metadata.clone(), to_re_digest(digest))
                        .await;

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

                tracing::debug!(target: "cas_client", "RichCasClient fetching {} {}(s)", digests.len(), log_name);

                let (data, stats) = {
                    let response = self.client()?
                        .co_low_level_download_inline(self.metadata.clone(), digests.iter().map(to_re_digest).collect()).await;

                    if let Err(ref err) = response {
                        if err.code == TCode::NOT_FOUND {
                            tracing::warn!(target: "cas_client", "digest not found and can not be fetched: {:?}", digests);
                        }
                    }

                    let response = response.inspect_err(|_err| {
                            // Unfortunately, the download failed entirely, record a failure.
                            let _failure_error = self.cas_success_tracker.record_failure();
                        })?;

                    let local_cache_stats = response.get_local_cache_stats();
                    let storage_stats = response.get_storage_stats();
                    (response.unpack_downloads(),  parse_stats(storage_stats.per_backend_stats.into_iter(), local_cache_stats))
                };


                let data = data
                    .into_iter()
                    .map(|blob| {
                        let (digest, status, data) = {
                            let (digest, status, data) = blob.unpack();
                            (digest, status, Blob::IOBuf(data.into()))
                        };

                        let digest = from_re_digest(&digest)?;
                        match status.code {
                            TCode::OK => Ok((digest, Ok(Some(data)))),
                            TCode::NOT_FOUND => Ok((digest, Ok(None))),
                            _ => Ok((
                                digest,
                                Err(anyhow!(
                                    "bad status (code={}, message={}, group={})",
                                    status.code,
                                    status.message,
                                    status.group
                                )),
                            )),
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;

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
        _fctx: FetchContext,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<'a, Result<(CasFetchedStats, Vec<CasDigest>, Vec<CasDigest>)>> {
        stream::iter(split_up_to_max_bytes(digests, self.fetch_limit.value()))
        .map(move |digests| async move {
            if !self.cas_success_tracker.allow_request()? {
                tracing::debug!(target: "cas_client", "RichCasClient skip prefetching {} {}(s)", digests.len(), log_name);
                return Err(anyhow!("skip cas prefetching due to cas success tracker error rate limiting violation"));
            }

            tracing::debug!(target: "cas_client", "RichCasClient prefetching {} {}(s)", digests.len(), log_name);

            let response = self.client()?
                .co_download_digests_into_cache(self.metadata.clone(), digests.iter().map(to_re_digest).collect())
                .await
                .inspect_err(|_err| {
                    // Unfortunately, the "download_digests_into_cache" failed entirely, record a failure.
                    let _failure_error = self.cas_success_tracker.record_failure();
                })?;

            let local_cache_stats = response.local_cache_stats;

            let stats = parse_stats(response.storage_stats.per_backend_stats.into_iter(), local_cache_stats);

            let data = response.digests_with_status
                .into_iter()
                .map(|blob| {
                    let digest = from_re_digest(&blob.digest)?;
                    match blob.status.code {
                        TCode::OK => Ok(CasPrefetchOutcome::Prefetched(digest)),
                        TCode::NOT_FOUND => {
                            tracing::warn!(target: "cas_client", "digest not found and can not be prefetched: {:?}", digest);
                            Ok(CasPrefetchOutcome::Missing(digest))
                        },
                        _ => Err(anyhow!(
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
            if data.is_err() {
                self.cas_success_tracker.record_failure()?;
            } else {
                self.cas_success_tracker.record_success();
            }

            let (digests_prefetched, digests_not_found) = data?.into_iter()
                .partition_map(|outcome| match outcome {
                    CasPrefetchOutcome::Prefetched(digest) => Either::Left(digest),
                    CasPrefetchOutcome::Missing(digest) => Either::Right(digest),
                });

            Ok((stats, digests_prefetched, digests_not_found))
        })
        .buffer_unordered(self.fetch_concurrency)
        .boxed()
    }
}
