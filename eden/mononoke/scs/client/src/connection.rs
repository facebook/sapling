/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Connection management.

use std::net::ToSocketAddrs;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use clientinfo::CLIENT_INFO_HEADER;
use fbinit::FacebookInit;
use sharding_ext::encode_repo_name;
use source_control::client::SourceControlService;
use source_control_x2pclients::make_SourceControlService_x2pclient;

const DEFAULT_TIER: &str = "shardmanager:mononoke.scs";

const CONN_TIMEOUT_MS: u32 = 5000;
const RECV_TIMEOUT_MS: u32 = 30_000;

#[derive(Clone)]
pub(crate) struct Connection {
    client: Arc<dyn SourceControlService + Sync>,
}

impl Connection {
    /// Build a connection from a `host:port` string.
    #[cfg(not(target_os = "windows"))]
    pub fn from_host_port(fb: FacebookInit, host_port: impl AsRef<str>) -> Result<Self, Error> {
        use source_control_thriftclients::make_SourceControlService_thriftclient;

        let mut addrs = host_port.as_ref().to_socket_addrs()?;
        let addr = addrs.next().expect("no address found");
        let client = make_SourceControlService_thriftclient!(
            fb,
            from_sock_addr = addr,
            with_conn_timeout = CONN_TIMEOUT_MS,
            with_recv_timeout = RECV_TIMEOUT_MS,
            with_secure = true
        )?;
        Ok(Self { client })
    }

    /// Build a connection from a `host:port` string.
    #[cfg(target_os = "windows")]
    pub fn from_host_port(_fb: FacebookInit, _host_port: impl AsRef<str>) -> Result<Self, Error> {
        Err(anyhow!(
            "Connection to host and port is not supported on this platform"
        ))
    }

    /// Build a connection from a tier name via servicerouter.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn from_tier_name_via_sr(
        fb: FacebookInit,
        client_id: String,
        tier: impl AsRef<str>,
        shardmanager_domain: Option<&str>,
    ) -> Result<Self, Error> {
        use maplit::hashmap;
        use source_control_srclients::make_SourceControlService_srclient;
        use srclient::ClientParams;

        let client_info = ClientInfo::new_with_entry_point(ClientEntryPoint::ScsClient)?;
        let headers = hashmap! {
            String::from(CLIENT_INFO_HEADER) => client_info.to_json()?,
        };
        let conn_config = hashmap! {
            String::from("client_id") => client_id,
        };

        let client = if let Some(shardmanager_domain) = shardmanager_domain {
            let client_params = ClientParams::new()
                .with_shard_manager_domain(encode_repo_name(shardmanager_domain));
            make_SourceControlService_srclient!(
                fb,
                tiername = tier.as_ref(),
                with_conn_config = &conn_config,
                with_persistent_headers = headers,
                with_client_params = client_params,
            )?
        } else {
            make_SourceControlService_srclient!(
                fb,
                tiername = tier.as_ref(),
                with_conn_config = &conn_config,
                with_persistent_headers = headers,
            )?
        };

        Ok(Self { client })
    }

    /// Build a connection from a tier name via servicerouter.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn from_tier_name_via_sr(
        _fb: FacebookInit,
        _client_id: String,
        _tier: impl AsRef<str>,
        _shardmanager_domain: Option<&str>,
    ) -> Result<Self, Error> {
        Err(anyhow!(
            "Connection via ServiceRouter is not supported on this platform"
        ))
    }

    /// Build a connection from a tier name via x2p.
    pub fn from_tier_name_via_x2p(
        fb: FacebookInit,
        _client_id: String,
        tier: impl AsRef<str>,
        shardmanager_domain: Option<&str>,
    ) -> Result<Self, Error> {
        let client = if let Some(shardmanager_domain) = shardmanager_domain {
            make_SourceControlService_x2pclient!(
                fb,
                tiername = tier.as_ref(),
                with_shard_manager_domain = encode_repo_name(shardmanager_domain)
            )?
        } else {
            make_SourceControlService_x2pclient!(fb, tiername = tier.as_ref())?
        };

        Ok(Self { client })
    }

    /// Build a connection from a tier name.
    pub fn from_tier_name(
        fb: FacebookInit,
        client_id: String,
        tier: impl AsRef<str>,
        shardmanager_domain: Option<&str>,
    ) -> Result<Self, Error> {
        match x2pclient::get_env(fb) {
            x2pclient::Environment::Prod => {
                Self::from_tier_name_via_sr(fb, client_id, tier, shardmanager_domain)
            }
            x2pclient::Environment::Corp => {
                Self::from_tier_name_via_x2p(fb, client_id, tier, shardmanager_domain)
            }
            other_env => Err(anyhow!("{} not supported", other_env)),
        }
    }
}

#[derive(clap::Args)]
pub(super) struct ConnectionArgs {
    #[clap(long, default_value = "scsc-default-client", global = true)]
    /// Name of the client for quota attribution and logging.
    client_id: String,
    #[clap(long, short, default_value = DEFAULT_TIER, global = true)]
    /// Connect to SCS through given tier.
    tier: String,
    #[clap(long, short = 'H', conflicts_with = "tier", global = true)]
    /// Connect to SCS through a given host and port pair, format HOST:PORT.
    host: Option<String>,
}

impl ConnectionArgs {
    pub fn get_connection(
        &self,
        fb: FacebookInit,
        repo: Option<&str>,
    ) -> Result<Connection, Error> {
        if let Some(host_port) = &self.host {
            Connection::from_host_port(fb, host_port)
        } else {
            Connection::from_tier_name(fb, self.client_id.clone(), &self.tier, repo)
        }
    }
}

impl std::ops::Deref for Connection {
    type Target = dyn SourceControlService + Sync;
    fn deref(&self) -> &Self::Target {
        &*self.client
    }
}
