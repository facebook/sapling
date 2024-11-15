/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use cas_client::CasClient;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use configmodel::Config;
use configmodel::ConfigExt;
use re_client_lib::create_default_config;
use re_client_lib::ExternalCASDaemonAddress;
use re_client_lib::REClient;
use re_client_lib::REClientBuilder;
use re_client_lib::RemoteExecutionMetadata;

pub struct ThinCasClient {
    client: re_cas_common::OnceCell<REClient>,
    metadata: RemoteExecutionMetadata,
    connection_count: u32,
    port: Option<i32>,
    uds_path: Option<String>,
    verbose: bool,
    log_dir: Option<String>,
    fetch_limit: ByteCount,
    fetch_concurrency: usize,
    use_streaming_dowloads: bool,
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

        Ok(Some(Self {
            client: Default::default(),
            connection_count: config.get_or("cas", "connection-count", || 1)?,
            port: config.get_opt::<i32>("cas", "port")?,
            uds_path: config.get_opt("cas", "uds-path")?,
            verbose,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case,
                ..Default::default()
            },
            log_dir,
            fetch_limit: config
                .get_or::<ByteCount>("cas", "max-batch-bytes", || default_fetch_limit)?,
            fetch_concurrency: config.get_or("cas", "fetch-concurrency", || 4)?,
            use_streaming_dowloads: config.get_or("cas", "use-streaming-downloads", || true)?,
        }))
    }

    fn build(&self) -> Result<REClient> {
        let start = Instant::now();
        let mut re_config = create_default_config();

        re_config.client_name = Some("sapling".to_string());
        re_config.quiet_mode = !self.verbose;
        re_config.log_file_location = self.log_dir.clone();
        re_config.features_config_path = "remote_execution/features/client_sapling".to_string();
        re_config.enable_ods_logging = false;
        re_config.enable_scuba_logging = false;
        re_config.enable_cancellation = true;

        let mut builder = REClientBuilder::new(fbinit::expect_init()).with_config(re_config);

        if let Some(port) = self.port {
            builder = builder
                .with_cas_daemon(ExternalCASDaemonAddress::port(port), self.connection_count);
        } else if let Some(uds_path) = self.uds_path.clone() {
            builder = builder.with_cas_daemon(
                ExternalCASDaemonAddress::uds_path(uds_path),
                self.connection_count,
            );
        } else {
            builder = builder.with_wdb_cas_daemon(self.connection_count);
        }

        let client = builder.build();
        let elapsed = start.elapsed();
        tracing::debug!(target: "cas", "creating RE CAS client took {} ms", elapsed.as_millis());
        client
    }
}

re_cas_common::re_client!(ThinCasClient);
