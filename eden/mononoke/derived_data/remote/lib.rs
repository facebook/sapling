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
    pub smc_tier: Option<String>,
}

/// Command line arguments for controlling remote derivation
#[derive(Args, Debug)]
pub struct RemoteDerivationArgs {
    /// Derive data remotely using the derived data service
    #[clap(long)]
    pub derive_remotely: bool,

    /// Specify SMC tier for the derived data service
    #[clap(long, value_name = "SMC")]
    pub derive_remotely_tier: Option<String>,
}

impl From<RemoteDerivationArgs> for RemoteDerivationOptions {
    fn from(args: RemoteDerivationArgs) -> Self {
        RemoteDerivationOptions {
            derive_remotely: args.derive_remotely,
            smc_tier: args.derive_remotely_tier,
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
