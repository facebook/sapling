/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use blob::Blob;
use cas_client::CasClient;
use cas_client::CasSuccessTracker;
use cas_client::CasSuccessTrackerConfig;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use re_client_lib::CASDaemonClientCfg;
use re_client_lib::ClientBuilderCommonMethods;
use re_client_lib::EmbeddedCASDaemonClientCfg;
#[cfg(not(target_os = "linux"))]
use re_client_lib::REClient;
#[cfg(not(target_os = "linux"))]
use re_client_lib::REClientBuilder;
use re_client_lib::RESessionID;
use re_client_lib::RemoteCASdAddress;
use re_client_lib::RemoteCacheConfig;
use re_client_lib::RemoteCacheManagerMode;
use re_client_lib::RemoteExecutionMetadata;
use re_client_lib::RemoteFetchPolicy;
use re_client_lib::create_default_config;
#[cfg(target_os = "linux")]
use rich_cas_client_wrapper::CASClientWrapper as REClient;

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
    client: re_cas_common::OnceCell<REClient>,
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
    use_streaming_dowloads: bool,
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
        // Kill switch in case something unexpected happens during construction of client.
        if config.get_or_default("cas", "disable")? {
            tracing::warn!(target: "cas", "disabled (cas.disable=true)");
            return Ok(None);
        }

        tracing::debug!(target: "cas", "creating rich client");
        RichCasClient::from_config(config).map(|c| c.map(|c| Arc::new(c) as Arc<dyn CasClient>))
    }
    factory::register_constructor("rich-client", construct);
}

impl RichCasClient {
    pub fn from_config(config: &dyn Config) -> Result<Option<Self>> {
        let use_case: String = match config.get("cas", "use-case") {
            Some(use_case) => use_case.to_string(),
            None => {
                let repo_name =
                    match config.get_nonempty_opt::<String>("remotefilelog", "reponame")? {
                        Some(repo_name) => repo_name,
                        None => {
                            tracing::info!(target: "cas", "no use case or repo name configured");
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
            .get_or("cas", "log-dir", || {
                Some("/tmp/eden_cas_client_logs".to_owned())
            })
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
            verbose: config.get_or_default("cas", "verbose")?,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case,
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
            use_streaming_dowloads: config.get_or("cas", "use-streaming-downloads", || true)?,
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

    fn build(&self) -> Result<REClient> {
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

        #[cfg(target_os = "linux")]
        let client = REClient::new(self.session_id.clone(), CAS_SESSION_TTL, re_config)?;
        #[cfg(not(target_os = "linux"))]
        let client = REClientBuilder::new(fbinit::expect_init())
            .with_config(re_config)
            .with_session_ttl(CAS_SESSION_TTL)
            .build()?;

        Ok(client)
    }
}

re_cas_common::re_client!(RichCasClient);
