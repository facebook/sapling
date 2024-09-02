/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Raw SCS Client.

use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use clientinfo::CLIENT_INFO_HEADER;
use fbinit::FacebookInit;
#[cfg(not(target_os = "windows"))]
use identity::IdentitySet;
use maplit::hashmap;
use sharding_ext::encode_repo_name;
pub use source_control as thrift;
use source_control_clients::SourceControlService;
use source_control_x2pclients::build_SourceControlService_client;

pub const SCS_DEFAULT_TIER: &str = "shardmanager:mononoke.scs";

#[cfg(not(target_os = "windows"))]
const CONN_TIMEOUT_MS: u32 = 5000;
#[cfg(not(target_os = "windows"))]
const RECV_TIMEOUT_MS: u32 = 30_000;

pub struct ScsClientBuilder {
    fb: FacebookInit,
    client_id: String,
    tier: String,
    repo: Option<String>,
    single_host: Option<SocketAddr>,
    processing_timeout: Option<Duration>,
}

impl ScsClientBuilder {
    pub fn new(fb: FacebookInit, client_id: String) -> Self {
        Self {
            fb,
            client_id,
            tier: SCS_DEFAULT_TIER.to_string(),
            repo: None,
            single_host: None,
            processing_timeout: None,
        }
    }

    pub fn with_repo(mut self, repo: Option<String>) -> Self {
        self.repo = repo;
        self
    }

    pub fn with_tier(mut self, tier: impl AsRef<str>) -> Self {
        self.tier = tier.as_ref().to_string();
        self
    }

    pub fn with_host_and_port(mut self, host_and_port: Option<String>) -> Result<Self> {
        if let Some(host_and_port) = host_and_port {
            let mut addrs = host_and_port.to_socket_addrs()?;
            let addr = addrs.next().expect("no address found");
            self.single_host = Some(addr);
        }
        Ok(self)
    }

    pub fn with_processing_timeout(mut self, processing_timeout_ms: Option<u64>) -> Self {
        self.processing_timeout = processing_timeout_ms.map(Duration::from_millis);
        self
    }

    pub fn build(self) -> Result<ScsClient, Error> {
        build_from_tier_name(
            self.fb,
            self.client_id,
            self.tier,
            self.repo.clone(),
            self.single_host,
            self.processing_timeout,
        )
    }
}

/// Build a scsclient from a tier name via servicerouter.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn build_from_tier_name_via_sr(
    fb: FacebookInit,
    client_id: String,
    tier: impl AsRef<str>,
    shardmanager_domain: Option<String>,
    single_host: Option<SocketAddr>,
    processing_timeout: Option<Duration>,
) -> Result<ScsClient, Error> {
    use source_control_srclients::make_SourceControlService_srclient;
    use srclient::ClientParams;

    let client_info = ClientInfo::new_with_entry_point(ClientEntryPoint::ScsClient)?;
    let correlator = client_info
        .request_info
        .as_ref()
        .map(|request_info| request_info.correlator.clone());
    let headers = hashmap! {
        String::from(CLIENT_INFO_HEADER) => client_info.to_json()?,
    };

    let client_params = ClientParams::new()
        .with_client_id(client_id)
        .maybe_with(correlator.clone(), |c, correlator| {
            c.with_logging_context(correlator)
        })
        .maybe_with(shardmanager_domain, |c, shardmanager_domain| {
            c.with_shard_manager_domain(encode_repo_name(&shardmanager_domain))
        })
        .maybe_with(single_host, |c, single_host| {
            c.with_single_host(single_host, None)
        })
        .maybe_with(processing_timeout, |c, processing_timeout| {
            c.with_processing_timeout(processing_timeout)
        });

    let client = make_SourceControlService_srclient!(
        fb,
        tiername = tier.as_ref(),
        with_persistent_headers = headers,
        with_client_params = client_params,
    )?;

    Ok(ScsClient { client, correlator })
}

/// Build a scsclient from a tier name via servicerouter.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn build_from_tier_name_via_sr(
    _fb: FacebookInit,
    _client_id: String,
    _tier: impl AsRef<str>,
    _shardmanager_domain: Option<String>,
    _single_host: Option<SocketAddr>,
    _processing_timeout: Option<Duration>,
) -> Result<ScsClient, Error> {
    Err(anyhow!(
        "Connection via ServiceRouter is not supported on this platform"
    ))
}

/// Build a scsclient from a tier name via x2p.
fn build_from_tier_name_via_x2p(
    fb: FacebookInit,
    client_id: String,
    tier: impl AsRef<str>,
    shardmanager_domain: Option<String>,
    single_host: Option<SocketAddr>,
    _processing_timeout: Option<Duration>,
) -> Result<ScsClient, Error> {
    let client_info = ClientInfo::new_with_entry_point(ClientEntryPoint::ScsClient)?;
    let headers = hashmap! {
        String::from(CLIENT_INFO_HEADER) => client_info.to_json()?,
    };

    let channel = x2pclient::X2pClientBuilder::from_service_name(fb, tier.as_ref())
        .with_client_id(client_id)
        .with_persistent_headers(headers)
        .maybe_with(shardmanager_domain, |c, shardmanager_domain| {
            c.with_shard_manager_domain(encode_repo_name(&shardmanager_domain))
        })
        .maybe_with(single_host, |c, single_host| c.with_host(single_host));
    let client = build_SourceControlService_client(channel)?;

    Ok(ScsClient {
        client,
        correlator: None,
    })
}

/// Build a scsclient from a tier name.
fn build_from_tier_name(
    fb: FacebookInit,
    client_id: String,
    tier: impl AsRef<str>,
    shardmanager_domain: Option<String>,
    single_host: Option<SocketAddr>,
    processing_timeout: Option<Duration>,
) -> Result<ScsClient, Error> {
    match x2pclient::get_env(fb) {
        x2pclient::Environment::Prod => {
            if cfg!(target_os = "linux") {
                build_from_tier_name_via_sr(
                    fb,
                    client_id,
                    tier,
                    shardmanager_domain,
                    single_host,
                    processing_timeout,
                )
            } else {
                build_from_tier_name_via_x2p(
                    fb,
                    client_id,
                    tier,
                    shardmanager_domain,
                    single_host,
                    processing_timeout,
                )
            }
        }
        x2pclient::Environment::Corp => build_from_tier_name_via_x2p(
            fb,
            client_id,
            tier,
            shardmanager_domain,
            single_host,
            processing_timeout,
        ),
        other_env => Err(anyhow!("{} not supported", other_env)),
    }
}

pub struct ScsClientHostBuilder {}

impl ScsClientHostBuilder {
    pub fn new() -> Self {
        Self {}
    }

    /// Build a scsclient from a `host:port` string.
    #[cfg(not(target_os = "windows"))]
    pub fn build_from_host_port(
        self,
        fb: FacebookInit,
        host_port: impl AsRef<str>,
    ) -> Result<ScsClient, Error> {
        use source_control_thriftclients::make_SourceControlService_thriftclient;

        let expected_identities = if let Ok(identity) =
            std::env::var("MONONOKE_INTEGRATION_TEST_EXPECTED_THRIFT_SERVER_IDENTITY")
        {
            IdentitySet::from_iter(std::iter::once(identity.parse()?))
        } else {
            IdentitySet::new()
        };

        let mut addrs = host_port.as_ref().to_socket_addrs()?;
        let addr = addrs.next().expect("no address found");
        let client = make_SourceControlService_thriftclient!(
            fb,
            from_sock_addr = addr,
            with_conn_timeout = CONN_TIMEOUT_MS,
            with_recv_timeout = RECV_TIMEOUT_MS,
            with_secure = true,
            with_expected_identities = expected_identities,
        )?;
        Ok(ScsClient {
            client,
            correlator: None,
        })
    }

    /// Build a scsclient from a `host:port` string.
    #[cfg(target_os = "windows")]
    pub fn build_from_host_port(
        self,
        _fb: FacebookInit,
        _host_port: impl AsRef<str>,
    ) -> Result<ScsClient, Error> {
        Err(anyhow!(
            "Connection to host and port is not supported on this platform"
        ))
    }
}

#[derive(Clone)]
pub struct ScsClient {
    client: Arc<dyn SourceControlService + Sync>,
    correlator: Option<String>,
}

impl ScsClient {
    /// Return the correlator for this scsclient.
    pub fn get_client_corrrelator(&self) -> Option<String> {
        self.correlator.clone()
    }
}

impl std::ops::Deref for ScsClient {
    type Target = dyn SourceControlService + Sync;
    fn deref(&self) -> &Self::Target {
        &*self.client
    }
}

#[cfg(not(any(target_os = "macos")))]
trait MaybeWith<T> {
    fn maybe_with<S>(self, optional: Option<S>, f: impl FnOnce(T, S) -> Self) -> Self
    where
        S: Sized;
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl MaybeWith<srclient::ClientParams> for srclient::ClientParams {
    fn maybe_with<S>(
        self,
        optional: Option<S>,
        f: impl FnOnce(srclient::ClientParams, S) -> Self,
    ) -> Self
    where
        S: Sized,
    {
        if let Some(s) = optional {
            f(self, s)
        } else {
            self
        }
    }
}

#[cfg(not(any(target_os = "macos")))]
impl MaybeWith<x2pclient::X2pClientBuilder> for x2pclient::X2pClientBuilder {
    fn maybe_with<S>(
        self,
        optional: Option<S>,
        f: impl FnOnce(x2pclient::X2pClientBuilder, S) -> Self,
    ) -> Self
    where
        S: Sized,
    {
        if let Some(s) = optional {
            f(self, s)
        } else {
            self
        }
    }
}
