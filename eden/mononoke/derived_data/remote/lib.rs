/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use clap::Args;
use derived_data_service_if::types as thrift;
use derived_data_service_if::types::DerivationType;
use mononoke_types::ChangesetId;

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
#[derive(Args, Debug)]
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

#[async_trait]
pub trait DerivationClient: Send + Sync {
    async fn derive_remotely(
        &self,
        repo_name: String,
        derived_data_type: String,
        cs_id: ChangesetId,
        config_name: String,
        derivation_type: DerivationType,
    ) -> Result<Option<thrift::DerivedData>>;
}
