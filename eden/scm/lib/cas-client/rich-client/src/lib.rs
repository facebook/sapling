/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use cas_client::CasClient;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use configmodel::Config;
use configmodel::ConfigExt;
use re_client_lib::create_default_config;
use re_client_lib::CASDaemonClientCfg;
use re_client_lib::EmbeddedCASDaemonClientCfg;
use re_client_lib::REClient;
use re_client_lib::REClientBuilder;
use re_client_lib::RemoteCASdAddress;
use re_client_lib::RemoteCacheConfig;
use re_client_lib::RemoteCacheManagerMode;
use re_client_lib::RemoteExecutionMetadata;
use re_client_lib::RemoteFetchPolicy;

pub const CAS_SOCKET_PATH: &str = "/run/casd/casd.socket";

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
    verbose: bool,
    metadata: RemoteExecutionMetadata,
    use_casd_cache: bool,
    cas_cache_mode_local_fetch: Option<CasCacheModeLocalFetch>,
    fetch_limit: ByteCount,
    fetch_concurrency: usize,
    use_streaming_dowloads: bool,
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

        let mut cas_cache_mode_local_fetch = None;

        if use_casd_cache {
            cas_cache_mode_local_fetch = Some(CasCacheModeLocalFetch::AllRemote);

            if config.get_or("cas", "shared_cache.local.small_files", || false)? {
                cas_cache_mode_local_fetch = Some(CasCacheModeLocalFetch::SmallFilesLocally);
            }

            if config.get_or("cas", "shared_cache.local.all_files", || false)? {
                cas_cache_mode_local_fetch = Some(CasCacheModeLocalFetch::AllFilesLocally);
            }
        }

        let default_fetch_limit = ByteCount::try_from_str("200MB")?;

        Ok(Some(Self {
            client: Default::default(),
            verbose: config.get_or_default("cas", "verbose")?,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case,
                ..Default::default()
            },
            use_casd_cache,
            cas_cache_mode_local_fetch,
            fetch_limit: config
                .get_or::<ByteCount>("cas", "max-batch-bytes", || default_fetch_limit)?,
            fetch_concurrency: config.get_or("cas", "fetch-concurrency", || 4)?,
            use_streaming_dowloads: config.get_or("cas", "use-streaming-downloads", || true)?,
        }))
    }

    fn build(&self) -> Result<REClient> {
        let mut re_config = create_default_config();

        re_config.client_name = Some("sapling".to_string());
        re_config.quiet_mode = !self.verbose;
        re_config.features_config_path = "remote_execution/features/client_eden".to_string();

        let mut embedded_config = EmbeddedCASDaemonClientCfg {
            name: "source_control".to_string(),
            ..Default::default()
        };
        if self.use_casd_cache {
            let mut remote_cache_config = RemoteCacheConfig {
                address: RemoteCASdAddress::uds_path(CAS_SOCKET_PATH.to_string()),
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
                        remote_cache_config.mode = RemoteCacheManagerMode::ALL_FILES;
                        remote_cache_config.small_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC;
                        remote_cache_config.large_files = RemoteFetchPolicy::REMOTE_FETCH;
                    }
                    CasCacheModeLocalFetch::AllRemote => {
                        remote_cache_config.mode = RemoteCacheManagerMode::ALL_FILES;
                        remote_cache_config.small_files = RemoteFetchPolicy::REMOTE_FETCH;
                        remote_cache_config.large_files = RemoteFetchPolicy::REMOTE_FETCH;
                    }
                }
            }
            embedded_config.remote_cache_config = Some(remote_cache_config);
            embedded_config.cache_config.writable_cache = false;
        }
        re_config.cas_client_config = CASDaemonClientCfg::embedded_config(embedded_config);

        let builder = REClientBuilder::new(fbinit::expect_init())
            .with_config(re_config)
            .with_rich_client(true);

        builder.build()
    }
}

re_cas_common::re_client!(RichCasClient);
