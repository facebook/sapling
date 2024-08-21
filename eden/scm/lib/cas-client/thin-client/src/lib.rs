/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use cas_client::CasClient;
use configmodel::Config;
use configmodel::ConfigExt;
use re_client_lib::create_default_config;
use re_client_lib::ExternalCASDaemonAddress;
use re_client_lib::REClient;
use re_client_lib::REClientBuilder;
use re_client_lib::RemoteExecutionMetadata;

pub struct ThinCasClient {
    client: REClient,
    metadata: RemoteExecutionMetadata,
}

pub fn init() {
    fn construct(config: &dyn Config) -> Result<Option<Arc<dyn CasClient>>> {
        // Kill switch in case something unexpected happens during construction of client.
        if config.get_or_default("cas", "disable")? {
            tracing::warn!(target: "cas", "disabled (cas.disable=true)");
            return Ok(None);
        }

        tracing::debug!(target: "cas", "creating thin client");
        ThinCasClient::from_config(config).map(|c| Some(Arc::new(c) as Arc<dyn CasClient>))
    }
    factory::register_constructor("thin-client", construct);
}

impl ThinCasClient {
    pub fn from_config(config: &dyn Config) -> Result<Self> {
        let mut re_config = create_default_config();

        re_config.client_name = Some("sapling".to_string());
        re_config.quiet_mode = !config.get_or_default("cas", "verbose")?;
        re_config.features_config_path = "remote_execution/features/client_sapling".to_string();

        let mut builder = REClientBuilder::new(fbinit::expect_init()).with_config(re_config);

        let connection_count: u32 = config.get_or("cas", "connection-count", || 1)?;

        if let Some(port) = config.get_opt::<i32>("cas", "port")? {
            builder =
                builder.with_cas_daemon(ExternalCASDaemonAddress::port(port), connection_count);
        } else if let Some(uds_path) = config.get_opt::<String>("cas", "uds-path")? {
            builder = builder.with_cas_daemon(
                ExternalCASDaemonAddress::uds_path(uds_path),
                connection_count,
            );
        } else {
            builder = builder.with_wdb_cas_daemon(connection_count);
        }

        let use_case: String = match config.get("cas", "use-case") {
            Some(use_case) => use_case.to_string(),
            None => format!(
                "source-control-{}",
                config.must_get::<String>("remotefilelog", "reponame")?
            ),
        };

        Ok(Self {
            client: builder.build()?,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case,
                ..Default::default()
            },
        })
    }
}

re_cas_common::re_client!(ThinCasClient);
