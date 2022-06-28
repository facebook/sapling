/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive::RootDeletedManifestDeriver;
use crate::mapping::RootDeletedManifestIdCommon;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use mononoke_types::deleted_manifest_v2::DeletedManifestV2;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DeletedManifestV2Id;
use std::collections::HashMap;
use unodes::RootUnodeManifestId;

use derived_data_service_if::types as thrift;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct RootDeletedManifestV2Id(DeletedManifestV2Id);

impl RootDeletedManifestIdCommon for RootDeletedManifestV2Id {
    type Manifest = DeletedManifestV2;
    type Id = DeletedManifestV2Id;

    fn id(&self) -> &Self::Id {
        &self.0
    }

    fn new(id: Self::Id) -> Self {
        Self(id)
    }

    fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
        let root_prefix = "derived_root_deleted_manifest2.";
        let key_prefix = derivation_ctx.mapping_key_prefix::<RootDeletedManifestV2Id>();
        format!("{}{}{}", root_prefix, key_prefix, changeset_id)
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

#[async_trait]
impl BonsaiDerivable for RootDeletedManifestV2Id {
    const NAME: &'static str = "deleted_manifest";

    type Dependencies = dependencies![RootUnodeManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        RootDeletedManifestDeriver::derive_single(ctx, derivation_ctx, bonsai, parents).await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        RootDeletedManifestDeriver::store_mapping(self, ctx, derivation_ctx, changeset_id).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        RootDeletedManifestDeriver::fetch(ctx, derivation_ctx, changeset_id).await
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        RootDeletedManifestDeriver::derive_batch(ctx, derivation_ctx, bonsais, gap_size).await
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
