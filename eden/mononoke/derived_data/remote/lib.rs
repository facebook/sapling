/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use derived_data_service_if::types as thrift;
use mononoke_types::ChangesetId;

#[derive(Clone, Debug)]
pub struct RemoteDerivationOptions {
    pub derive_remotely: bool,
    pub smc_tier: Option<String>,
}

#[async_trait]
pub trait DerivationClient: Send + Sync {
    async fn derive_remotely(
        &self,
        repo_name: String,
        derived_data_type: String,
        cs_id: ChangesetId,
        config_name: String,
    ) -> Result<Option<thrift::DerivedData>>;
}
