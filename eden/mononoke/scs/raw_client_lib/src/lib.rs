/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Raw SCS Client.

use std::net::ToSocketAddrs;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use clientinfo::CLIENT_INFO_HEADER;
use fbinit::FacebookInit;
use identity::IdentitySet;
use maplit::hashmap;
use sharding_ext::encode_repo_name;
pub use source_control as thrift;
use source_control_clients::SourceControlService;
use source_control_x2pclients::make_SourceControlService_x2pclient;

pub const SCS_DEFAULT_TIER: &str = "shardmanager:mononoke.scs";

const CONN_TIMEOUT_MS: u32 = 5000;
const RECV_TIMEOUT_MS: u32 = 30_000;

pub struct ScsClientBuilder {}

impl ScsClientBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build(
        self,
        fb: FacebookInit,
        client_id: String,
        repo: Option<&str>,
    ) -> Result<ScsClient, Error> {
        self.build_from_tier_name(fb, client_id, SCS_DEFAULT_TIER, repo)
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

    /// Build a scsclient from a tier name via servicerouter.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn build_from_tier_name_via_sr(
        self,
        fb: FacebookInit,
        client_id: String,
        tier: impl AsRef<str>,
        shardmanager_domain: Option<&str>,
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
                c.with_shard_manager_domain(encode_repo_name(shardmanager_domain))
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
    pub fn build_from_tier_name_via_sr(
        self,
        _fb: FacebookInit,
        _client_id: String,
        _tier: impl AsRef<str>,
        _shardmanager_domain: Option<&str>,
    ) -> Result<ScsClient, Error> {
        Err(anyhow!(
            "Connection via ServiceRouter is not supported on this platform"
        ))
    }

    /// Build a scsclient from a tier name via x2p.
    pub fn build_from_tier_name_via_x2p(
        self,
        fb: FacebookInit,
        client_id: String,
        tier: impl AsRef<str>,
        shardmanager_domain: Option<&str>,
    ) -> Result<ScsClient, Error> {
        let client_info = ClientInfo::new_with_entry_point(ClientEntryPoint::ScsClient)?;
        let headers = hashmap! {
            String::from(CLIENT_INFO_HEADER) => client_info.to_json()?,
        };
        let client = if let Some(shardmanager_domain) = shardmanager_domain {
            make_SourceControlService_x2pclient!(
                fb,
                tiername = tier.as_ref(),
                with_client_id = client_id,
                with_persistent_headers = headers,
                with_shard_manager_domain = encode_repo_name(shardmanager_domain)
            )?
        } else {
            make_SourceControlService_x2pclient!(
                fb,
                tiername = tier.as_ref(),
                with_client_id = client_id,
                with_persistent_headers = headers,
            )?
        };

        Ok(ScsClient {
            client,
            correlator: None,
        })
    }

    /// Build a scsclient from a tier name.
    pub fn build_from_tier_name(
        self,
        fb: FacebookInit,
        client_id: String,
        tier: impl AsRef<str>,
        shardmanager_domain: Option<&str>,
    ) -> Result<ScsClient, Error> {
        match x2pclient::get_env(fb) {
            x2pclient::Environment::Prod => {
                if cfg!(target_os = "linux") {
                    self.build_from_tier_name_via_sr(fb, client_id, tier, shardmanager_domain)
                } else {
                    self.build_from_tier_name_via_x2p(fb, client_id, tier, shardmanager_domain)
                }
            }
            x2pclient::Environment::Corp => {
                self.build_from_tier_name_via_x2p(fb, client_id, tier, shardmanager_domain)
            }
            other_env => Err(anyhow!("{} not supported", other_env)),
        }
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
trait MaybeWith {
    fn maybe_with<S>(
        self,
        optional: Option<S>,
        f: impl FnOnce(srclient::ClientParams, S) -> Self,
    ) -> Self
    where
        S: ToString;
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl MaybeWith for srclient::ClientParams {
    fn maybe_with<S>(
        self,
        optional: Option<S>,
        f: impl FnOnce(srclient::ClientParams, S) -> Self,
    ) -> Self
    where
        S: ToString,
    {
        if let Some(s) = optional {
            f(self, s)
        } else {
            self
        }
    }
}
