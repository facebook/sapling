/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::MappedGitCommitId;
use crate::TreeHandle;

#[async_trait]
impl BonsaiDerivable for MappedGitCommitId {
    const VARIANT: DerivableType = DerivableType::GitCommit;

    type Dependencies = dependencies![TreeHandle];

    async fn derive_single(
        _ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self> {
        if bonsai.is_snapshot() {
            bail!("Can't derive MappedGitCommitId for snapshot")
        }
        let _blobstore = derivation_ctx.blobstore().clone();
        todo!()
    }

    async fn store_mapping(
        self,
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        _changeset_id: ChangesetId,
    ) -> Result<()> {
        todo!()
    }

    async fn fetch(
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        _changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        todo!()
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::commit_handle(
            thrift::DerivedDataCommitHandle::mapped_commit_id(id),
        ) = data
        {
            Self::try_from(id)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::commit_handle(
            thrift::DerivedDataCommitHandle::mapped_commit_id(data.into()),
        ))
    }
}

impl_bonsai_derived_via_manager!(MappedGitCommitId);
