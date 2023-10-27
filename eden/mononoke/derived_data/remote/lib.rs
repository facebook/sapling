/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use arg_extensions::ArgDefaults;
use async_trait::async_trait;
use clap::Args;
use context::CoreContext;
use derived_data_service_if::types as thrift;

#[derive(Clone, Debug)]
pub struct RemoteDerivationOptions {
    pub derive_remotely: bool,
    pub address: Address,
}

#[derive(Clone, Debug)]
pub enum Address {
    SmcTier(String),
    HostPort(String),
    Empty,
}

/// Command line arguments for controlling remote derivation
#[derive(Args, Debug, Clone)]
pub struct RemoteDerivationArgs {
    /// Derive data remotely using the derived data service
    #[clap(long)]
    pub derive_remotely: bool,

    /// Specify SMC tier for the derived data service
    #[clap(long, value_name = "SMC", group = "Address")]
    pub derive_remotely_tier: Option<String>,

    /// Specify Host:Port pair to connect to derived data service
    #[clap(long, value_name = "HOST:PORT", group = "Address")]
    pub derive_remotely_hostport: Option<String>,
}

impl ArgDefaults for RemoteDerivationArgs {
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        let mut args = vec![("derive_remotely", self.derive_remotely.to_string())];

        if let Some(derive_remotely_tier) = &self.derive_remotely_tier {
            args.push((
                "derive_remotely_tier",
                derive_remotely_tier.clone().to_string(),
            ));
        };

        if let Some(derive_remotely_hostport) = &self.derive_remotely_hostport {
            args.push((
                "derive_remotely_hostport",
                derive_remotely_hostport.clone().to_string(),
            ));
        };

        args
    }
}

impl From<RemoteDerivationArgs> for RemoteDerivationOptions {
    fn from(args: RemoteDerivationArgs) -> Self {
        let address = match (args.derive_remotely_tier, args.derive_remotely_hostport) {
            (Some(tier), _) => Address::SmcTier(tier),
            (_, Some(host_port)) => Address::HostPort(host_port),
            (_, _) => Address::Empty,
        };
        RemoteDerivationOptions {
            derive_remotely: args.derive_remotely,
            address,
        }
    }
}

impl RemoteDerivationOptions {
    pub fn from_args(args: &RemoteDerivationArgs) -> Self {
        From::<RemoteDerivationArgs>::from(args.clone())
    }
}

#[async_trait]
pub trait DerivationClient: Send + Sync {
    async fn derive_remotely(
        &self,
        ctx: &CoreContext,
        request: &thrift::DeriveRequest,
    ) -> Result<thrift::DeriveResponse>;

    async fn poll(
        &self,
        ctx: &CoreContext,
        request: &thrift::DeriveRequest,
    ) -> Result<thrift::DeriveResponse>;
}
