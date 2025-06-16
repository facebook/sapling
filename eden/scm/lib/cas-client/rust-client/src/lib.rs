/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use blob::Blob;
use cas_client::CasClient;
use cas_client::CasDigest;
use cas_client::CasDigestType;
use cas_client::CasFetchedStats;
use cas_client::FetchContext;
use cas_daemon_types_thriftclients::CASDaemonServiceClient;
use cas_daemon_types_thriftclients::make_CASDaemonService_thriftclient;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use futures::stream::BoxStream;
use once_cell::sync::OnceCell;
use thriftclient::TransportType;

#[allow(dead_code)]
pub struct RustCasClient {
    client: OnceCell<CASDaemonServiceClient>,
    port: Option<u16>,
    uds_path: Option<String>,
    use_case: String,
    session_id: String,
    fetch_limit: ByteCount,
    fetch_concurrency: usize,
    connection_timeout_ms: u32,
    recv_timeout_ms: u32,
}

pub fn init() {
    fn construct(config: &dyn Config) -> Result<Option<Arc<dyn CasClient>>> {
        // Kill switch in case something unexpected happens during construction of client.
        if config.get_or_default("cas", "disable")? {
            tracing::warn!(target: "cas", "disabled (cas.disable=true)");
            return Ok(None);
        }

        tracing::debug!(target: "cas", "creating rust client");
        RustCasClient::from_config(config).map(|c| c.map(|c| Arc::new(c) as Arc<dyn CasClient>))
    }
    factory::register_constructor("rust-cas-client", construct);
}

impl RustCasClient {
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

        let default_fetch_limit = ByteCount::try_from_str("200MB")?;
        let cri = clientinfo::get_client_request_info();
        let session_id = format!("{}_{}", cri.entry_point, cri.correlator);

        Ok(Some(Self {
            client: Default::default(),
            port: config.get_opt::<u16>("cas", "port")?,
            uds_path: config.get_opt("cas", "uds-path")?,
            use_case,
            session_id,
            fetch_limit: config
                .get_or::<ByteCount>("cas", "max-batch-bytes", || default_fetch_limit)?,
            fetch_concurrency: config.get_or("cas", "fetch-concurrency", || 4)?,
            connection_timeout_ms: config.get_or("cas", "connection-timeout-ms", || 500)?,
            recv_timeout_ms: config.get_or("cas", "recv-timeout-ms", || 500)?,
        }))
    }

    #[allow(dead_code)]
    fn client(&self) -> Result<&CASDaemonServiceClient> {
        self.client.get_or_try_init(|| {
            if !fbinit::was_performed() {
                return Err(anyhow::anyhow!("fbinit is required to create CAS client"));
            }

            if let Some(port) = self.port {
                make_CASDaemonService_thriftclient!(
                    fbinit::expect_init(),
                    from_sock_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port),
                    with_conn_timeout = self.connection_timeout_ms,
                    with_recv_timeout = self.recv_timeout_ms,
                    with_secure = false,
                    with_transport_type = TransportType::Rocket,
                    with_channel_pool = false,
                )
            } else {
                make_CASDaemonService_thriftclient!(
                    fbinit::expect_init(),
                    from_path = self.uds_path.as_deref().unwrap_or("/run/casd/casd.socket"),
                    with_conn_timeout = self.connection_timeout_ms,
                    with_recv_timeout = self.recv_timeout_ms,
                    with_secure = false,
                    with_transport_type = TransportType::Rocket,
                    with_channel_pool = false,
                )
            }
        })
    }
}

#[async_trait::async_trait]
impl CasClient for RustCasClient {
    fn fetch_single_locally_cached(
        &self,
        _digest: &CasDigest,
    ) -> Result<(CasFetchedStats, Option<Blob>)> {
        Ok((CasFetchedStats::default(), None))
    }

    async fn fetch<'a>(
        &'a self,
        _fctx: FetchContext,
        _digests: &'a [CasDigest],
        _log_name: CasDigestType,
    ) -> BoxStream<'a, Result<(CasFetchedStats, Vec<(CasDigest, Result<Option<Blob>>)>)>> {
        unimplemented!("CasClient::fetch is not implemented for RustCasClient")
    }

    async fn upload(&self, blobs: Vec<Blob>) -> Result<Vec<CasDigest>> {
        unimplemented!("CasClient::upload is not implemented for RustCasClient")
    }

    async fn prefetch<'a>(
        &'a self,
        _fctx: FetchContext,
        _digests: &'a [CasDigest],
        _log_name: CasDigestType,
    ) -> BoxStream<'a, Result<(CasFetchedStats, Vec<CasDigest>, Vec<CasDigest>)>> {
        unimplemented!("CasClient::prefetch is not implemented for RustCasClient")
    }
}
