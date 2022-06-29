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
use fbinit::FacebookInit;
use source_control::client::make_SourceControlService;
use source_control::client::SourceControlService;
use x2pclient::X2pClientBuilder;

const DEFAULT_TIER: &str = "mononoke-scs-server";

const CONN_TIMEOUT_MS: u32 = 1000;
const RECV_TIMEOUT_MS: u32 = 30_000;

#[derive(Clone)]
pub(crate) struct Connection {
    client: Arc<dyn SourceControlService + Sync>,
}

impl Connection {
    /// Build a connection from a `host:port` string.
    #[cfg(not(target_os = "windows"))]
    pub fn from_host_port(fb: FacebookInit, host_port: impl AsRef<str>) -> Result<Self, Error> {
        use thriftclient::ThriftChannelBuilder;

        let mut addrs = host_port.as_ref().to_socket_addrs()?;
        let addr = addrs.next().expect("no address found");
        let client = ThriftChannelBuilder::from_sock_addr(fb, addr)?
            .with_conn_timeout(CONN_TIMEOUT_MS)
            .with_recv_timeout(RECV_TIMEOUT_MS)
            .with_secure(true)
            .build_client(make_SourceControlService)?;
        Ok(Self { client })
    }

    /// Build a connection from a `host:port` string.
    #[cfg(target_os = "windows")]
    pub fn from_host_port(fb: FacebookInit, host_port: impl AsRef<str>) -> Result<Self, Error> {
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
    ) -> Result<Self, Error> {
        use maplit::hashmap;
        use rand::distributions::Alphanumeric;
        use rand::Rng;
        use srclient::SRChannelBuilder;

        let correlator: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let headers = hashmap! {
            String::from("client_type") => String::from("scsc CLI"),
            String::from("client_correlator") => correlator,
        };
        let conn_config = hashmap! {
            String::from("client_id") => client_id,
        };
        let client = SRChannelBuilder::from_service_name(fb, tier.as_ref())?
            .with_conn_config(&conn_config)
            .with_persistent_headers(headers)
            .build_client(make_SourceControlService)?;
        Ok(Self { client })
    }

    /// Build a connection from a tier name via servicerouter.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn from_tier_name_via_sr(
        _fb: FacebookInit,
        _client_id: String,
        _tier: impl AsRef<str>,
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
    ) -> Result<Self, Error> {
        let client = X2pClientBuilder::from_service_name(fb, tier.as_ref())
            .build_client(make_SourceControlService)?;
        Ok(Self { client })
    }

    /// Build a connection from a tier name.
    pub fn from_tier_name(
        fb: FacebookInit,
        client_id: String,
        tier: impl AsRef<str>,
    ) -> Result<Self, Error> {
        match x2pclient::get_env(fb) {
            x2pclient::Environment::Prod => Self::from_tier_name_via_sr(fb, client_id, tier),
            x2pclient::Environment::Corp => Self::from_tier_name_via_x2p(fb, client_id, tier),
            other_env => Err(anyhow!("{} not supported", other_env)),
        }
    }
}

#[derive(clap::Args)]
pub(super) struct ConnectionArgs {
    #[clap(long, default_value = "scsc-default-client")]
    /// Name of the client for quota attribution and logging.
    client_id: String,
    #[clap(long, short, default_value = DEFAULT_TIER)]
    /// Connect to SCS through given tier.
    tier: String,
    #[cfg(not(target_os = "windows"))]
    #[clap(long, short, conflicts_with = "tier")]
    /// Connect to SCS through a given host and port pair, format HOST:PORT.
    host: Option<String>,
}

impl ConnectionArgs {
    pub fn get_connection(&self, fb: FacebookInit) -> Result<Connection, Error> {
        if let Some(host_port) = &self.host {
            Connection::from_host_port(fb, host_port)
        } else {
            Connection::from_tier_name(fb, self.client_id.clone(), &self.tier)
        }
    }
}

impl std::ops::Deref for Connection {
    type Target = dyn SourceControlService + Sync;
    fn deref(&self) -> &Self::Target {
        &*self.client
    }
}
