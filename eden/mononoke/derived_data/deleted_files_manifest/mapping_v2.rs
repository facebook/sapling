/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive::{get_changes, DeletedManifestDeriver};
use crate::mapping::RootDeletedManifestIdCommon;
use anyhow::{anyhow, Context, Error, Result};
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData};
use bytes::Bytes;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::{dependencies, BonsaiDerivable, DerivationContext};
use mononoke_types::{
    deleted_manifest_v2::DeletedManifestV2, BlobstoreBytes, BonsaiChangeset, ChangesetId,
    DeletedManifestV2Id,
};
use unodes::RootUnodeManifestId;

use derived_data_service_if::types as thrift;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootDeletedManifestV2Id(DeletedManifestV2Id);

impl RootDeletedManifestIdCommon for RootDeletedManifestV2Id {
    type Manifest = DeletedManifestV2;
    type Id = DeletedManifestV2Id;

    fn id(&self) -> &Self::Id {
        &self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootDeletedManifestV2Id {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        DeletedManifestV2Id::from_bytes(&blob_bytes.into_bytes()).map(RootDeletedManifestV2Id)
    }
}

impl TryFrom<BlobstoreGetData> for RootDeletedManifestV2Id {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootDeletedManifestV2Id> for BlobstoreBytes {
    fn from(root_mf_id: RootDeletedManifestV2Id) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_mf_id.0.blake2().as_ref()))
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_deleted_manifest2.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootDeletedManifestV2Id>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for RootDeletedManifestV2Id {
    const NAME: &'static str = "deleted_manifest2";

    type Dependencies = dependencies![RootUnodeManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let bcs_id = bonsai.get_changeset_id();
        let changes = get_changes(ctx, derivation_ctx, bonsai).await?;
        let id = DeletedManifestDeriver::<DeletedManifestV2>::derive(
            ctx,
            derivation_ctx.blobstore(),
            bcs_id,
            parents
                .into_iter()
                .map(|root_mf_id| root_mf_id.id().clone())
                .collect(),
            changes,
        )
        .await
        .context("Deriving DMv2")?;
        Ok(RootDeletedManifestV2Id(id))
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
        if let thrift::DerivedData::deleted_manifest_v2(
            thrift::DerivedDataDeletedManifestV2::root_deleted_manifest_v2_id(id),
        ) = data
        {
            DeletedManifestV2Id::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::deleted_manifest_v2(
            thrift::DerivedDataDeletedManifestV2::root_deleted_manifest_v2_id(
                data.id().into_thrift(),
            ),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootDeletedManifestV2Id);

#[cfg(test)]
crate::test_utils::impl_deleted_manifest_tests!(RootDeletedManifestV2Id);
