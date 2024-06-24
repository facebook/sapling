/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if as thrift;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;

use crate::delta_manifest_v2::GitDeltaManifestV2Id;
use crate::MappedGitCommitId;
use crate::TreeHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RootGitDeltaManifestV2Id(GitDeltaManifestV2Id);

impl RootGitDeltaManifestV2Id {
    pub fn manifest_id(&self) -> &GitDeltaManifestV2Id {
        &self.0
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_gdm2_test3.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootGitDeltaManifestV2Id>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootGitDeltaManifestV2Id {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        GitDeltaManifestV2Id::from_bytes(&blob_bytes.into_bytes()).map(RootGitDeltaManifestV2Id)
    }
}

impl TryFrom<BlobstoreGetData> for RootGitDeltaManifestV2Id {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootGitDeltaManifestV2Id> for BlobstoreBytes {
    fn from(root_mf_id: RootGitDeltaManifestV2Id) -> Self {
        BlobstoreBytes::from_bytes(root_mf_id.0.into_bytes())
    }
}

async fn derive_single(
    _ctx: &CoreContext,
    _derivation_ctx: &DerivationContext,
    _bonsai: BonsaiChangeset,
) -> Result<RootGitDeltaManifestV2Id> {
    unimplemented!("git_delta_manifest_v2 derivation is not implemented")
}

#[async_trait]
impl BonsaiDerivable for RootGitDeltaManifestV2Id {
    const VARIANT: DerivableType = DerivableType::GitDeltaManifestsV2;

    type Dependencies = dependencies![TreeHandle, MappedGitCommitId];
    type PredecessorDependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai).await
    }

    async fn derive_from_predecessor(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai).await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx.blobstore().put(ctx, key, self.into()).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        Ok(derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()?)
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::git_delta_manifest_v2(
            thrift::DerivedDataGitDeltaManifestV2::root_git_delta_manifest_v2_id(id),
        ) = data
        {
            GitDeltaManifestV2Id::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::git_delta_manifest_v2(
            thrift::DerivedDataGitDeltaManifestV2::root_git_delta_manifest_v2_id(
                data.0.into_thrift(),
            ),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootGitDeltaManifestV2Id);
