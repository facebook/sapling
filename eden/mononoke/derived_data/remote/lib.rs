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
use derived_data_service_if as thrift;

#[derive(Clone, Debug)]
pub struct RemoteDerivationOptions {
    pub derive_remotely: bool,
}

/// Command line arguments for controlling remote derivation
#[derive(Args, Debug, Clone)]
pub struct RemoteDerivationArgs {
    /// Derive data remotely using the derived data service
    #[clap(long)]
    pub derive_remotely: bool,
}

impl ArgDefaults for RemoteDerivationArgs {
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        vec![("derive_remotely", self.derive_remotely.to_string())]
    }
}

impl From<RemoteDerivationArgs> for RemoteDerivationOptions {
    fn from(args: RemoteDerivationArgs) -> Self {
        RemoteDerivationOptions {
            derive_remotely: args.derive_remotely,
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
