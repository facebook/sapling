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
use clap::App;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use fbinit::FacebookInit;
use source_control::client::make_SourceControlService;
use source_control::client::SourceControlService;
use x2pclient::X2pClientBuilder;

const ARG_TIER: &str = "TIER";
const ARG_HOST_PORT: &str = "HOST:PORT";
const ARG_CLIENT_ID: &str = "CLIENT_ID";

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

    /// Build a connection from the specified arguments.
    pub fn from_args(fb: FacebookInit, matches: &ArgMatches) -> Result<Self, Error> {
        let client_id = matches
            .value_of(ARG_CLIENT_ID)
            .expect("client_id can't be null");
        if let Some(host_port) = matches.value_of(ARG_HOST_PORT) {
            Self::from_host_port(fb, host_port)
        } else {
            let tier = matches.value_of(ARG_TIER).unwrap_or(DEFAULT_TIER);
            Self::from_tier_name(fb, client_id.to_string(), tier)
        }
    }
}

impl std::ops::Deref for Connection {
    type Target = dyn SourceControlService + Sync;
    fn deref(&self) -> &Self::Target {
        &*self.client
    }
}

/// Add args for setting up the connection.
pub(crate) fn add_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    let app = app
        .arg(
            Arg::with_name(ARG_CLIENT_ID)
                .long("client-id")
                .global(true)
                .help("Name of the client for quota attribution and logging")
                .takes_value(true)
                .default_value("scsc-default-client"),
        )
        .arg(
            Arg::with_name(ARG_TIER)
                .short("t")
                .long("tier")
                .global(true)
                .help("Tier name to connect to")
                .takes_value(true),
        );

    if cfg!(not(target_os = "windows")) {
        app.arg(
            Arg::with_name(ARG_HOST_PORT)
                .short("h")
                .long("host")
                .global(true)
                .help("Host to connect to")
                .takes_value(true),
        )
        .group(ArgGroup::with_name("connection").args(&[ARG_TIER, ARG_HOST_PORT]))
    } else {
        app
    }
}
