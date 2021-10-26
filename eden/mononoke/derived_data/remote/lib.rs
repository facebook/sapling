/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::ChangesetId;

#[async_trait]
pub trait DerivationClient {
    type Output;

    async fn derive_remotely(
        &self,
        repo_name: String,
        derived_data_type: String,
        cs_id: ChangesetId,
    ) -> Result<Option<Self::Output>>;
}
