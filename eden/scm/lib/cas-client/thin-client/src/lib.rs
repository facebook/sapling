/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use cas_client::CasClient;
use cas_client::CasSuccessTracker;
use cas_client::CasSuccessTrackerConfig;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use re_client_lib::CASDaemonClientCfg;
use re_client_lib::ExternalCASDaemonAddress;
use re_client_lib::ExternalCASDaemonCfg;
#[cfg(not(target_os = "linux"))]
use re_client_lib::REClient;
#[cfg(not(target_os = "linux"))]
use re_client_lib::REClientBuilder;
use re_client_lib::RESessionID;
use re_client_lib::RemoteExecutionMetadata;
use re_client_lib::create_default_config;
use scm_blob::ScmBlob;
#[cfg(target_os = "linux")]
use thin_cas_client_wrapper::CASClientWrapper as REClient;

pub struct ThinCasClient {
    client: re_cas_common::OnceCell<REClient>,
    cas_success_tracker: CasSuccessTracker,
    metadata: RemoteExecutionMetadata,
    connection_count: u32,
    port: Option<i32>,
    uds_path: Option<String>,
    verbose: bool,
    log_dir: Option<String>,
    fetch_limit: ByteCount,
    fetch_concurrency: usize,
    use_streaming_dowloads: bool,
    session_id: String,
    use_persistent_caches: bool,
}

const DEFAULT_SCM_CAS_LOGS_DIR: &str = "scm_cas";

pub fn init() {
    fn construct(config: &dyn Config) -> Result<Option<Arc<dyn CasClient>>> {
        // Kill switch in case something unexpected happens during construction of client.
        if config.get_or_default("cas", "disable")? {
            tracing::warn!(target: "cas", "disabled (cas.disable=true)");
            return Ok(None);
        }

        tracing::debug!(target: "cas", "creating thin client");
        ThinCasClient::from_config(config).map(|c| c.map(|c| Arc::new(c) as Arc<dyn CasClient>))
    }
    factory::register_constructor("thin-client", construct);
}

impl ThinCasClient {
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

        let verbose: bool = config.get_or_default("cas", "verbose")?;
        let mut log_dir = None;
        if !verbose {
            // If we're not verbose, we don't want to log to stderr.
            log_dir = config.get_opt("cas", "log-dir")?;
            if let Some(ref log_dir_path) = log_dir {
                if !std::path::Path::new(log_dir_path).exists() {
                    if let Err(err) = std::fs::create_dir(log_dir_path) {
                        tracing::warn!(target: "cas", "failed to create log dir with path {}: {}", log_dir_path, err);
                        log_dir = None;
                    }
                }
            } else {
                let temp_dir = std::env::temp_dir();
                let log_dir_path = temp_dir.join(DEFAULT_SCM_CAS_LOGS_DIR);
                log_dir = Some(log_dir_path.to_string_lossy().to_string());
                if !std::path::Path::new(&log_dir_path).exists() {
                    if let Err(err) = std::fs::create_dir(&log_dir_path) {
                        tracing::warn!(target: "cas", "failed to create log dir with path {:?}: {}", log_dir_path, err);
                        log_dir = None;
                    }
                }
            }
        }

        let default_fetch_limit = ByteCount::try_from_str("200MB")?;
        let cri = clientinfo::get_client_request_info();
        let session_id = format!("{}_{}", cri.entry_point, cri.correlator);

        Ok(Some(Self {
            client: Default::default(),
            connection_count: config.get_or("cas", "connection-count", || 1)?,
            port: config.get_opt::<i32>("cas", "port")?,
            uds_path: config.get_opt("cas", "uds-path")?,
            verbose,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case,
                re_session_id: Some(RESessionID {
                    id: session_id.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            log_dir,
            session_id,
            fetch_limit: config
                .get_or::<ByteCount>("cas", "max-batch-bytes", || default_fetch_limit)?,
            fetch_concurrency: config.get_or("cas", "fetch-concurrency", || 4)?,
            use_streaming_dowloads: config.get_or("cas", "use-streaming-downloads", || true)?,
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
        re_config.features_config_path = "remote_execution/features/client_sapling".to_string();
        re_config.enable_ods_logging = false;
        re_config.enable_scuba_logging = false;
        re_config.enable_cancellation = true;

        let mut external_config = if let Some(port) = self.port {
            ExternalCASDaemonCfg {
                cas_daemon_port: port,
                cas_daemon_address: ExternalCASDaemonAddress::port(port),
                address: None,
                connection_count: self.connection_count as i32,
                ..Default::default()
            }
        } else if let Some(uds_path) = self.uds_path.clone() {
            ExternalCASDaemonCfg {
                cas_daemon_port: 0,
                cas_daemon_address: ExternalCASDaemonAddress::uds_path(uds_path),
                address: None,
                connection_count: self.connection_count as i32,
                ..Default::default()
            }
        } else {
            let socket_path = std::env::var("CASD_SOCKET_PATH")
                .unwrap_or(re_client_lib::DEFAULT_CASD_SOCKET.to_string());
            ExternalCASDaemonCfg {
                address: None,
                connection_count: self.connection_count as i32,
                cas_daemon_address: ExternalCASDaemonAddress::uds_path(socket_path),
                socket_activation: true,
                ..Default::default()
            }
        };

        if self.use_persistent_caches {
            external_config.client_label = "pc_enabled".to_string();
        }

        re_config.cas_client_config = CASDaemonClientCfg::external_config(external_config);

        #[cfg(target_os = "linux")]
        let client = REClient::new(self.session_id.clone(), 0, re_config)?;
        #[cfg(not(target_os = "linux"))]
        let client = REClientBuilder::new(fbinit::expect_init())
            .with_config(re_config)
            .build()?;

        Ok(client)
    }
}

re_cas_common::re_client!(ThinCasClient);
