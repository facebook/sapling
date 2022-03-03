/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive::{get_changes, DeletedManifestDeriver};
use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData};
use bytes::Bytes;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::{dependencies, BonsaiDerivable, DerivationContext};
use mononoke_types::{
    deleted_files_manifest::DeletedManifest, BlobstoreBytes, BonsaiChangeset, ChangesetId,
    DeletedManifestId,
};
use unodes::RootUnodeManifestId;

use derived_data_service_if::types as thrift;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootDeletedManifestId(DeletedManifestId);

impl RootDeletedManifestId {
    pub fn deleted_manifest_id(&self) -> &DeletedManifestId {
        &self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootDeletedManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        DeletedManifestId::from_bytes(&blob_bytes.into_bytes()).map(RootDeletedManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootDeletedManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootDeletedManifestId> for BlobstoreBytes {
    fn from(root_mf_id: RootDeletedManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_mf_id.0.blake2().as_ref()))
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_deleted_manifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootDeletedManifestId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for RootDeletedManifestId {
    const NAME: &'static str = "deleted_manifest";

    type Dependencies = dependencies![RootUnodeManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let bcs_id = bonsai.get_changeset_id();
        let changes = get_changes(ctx, derivation_ctx, bonsai).await?;
        let id = DeletedManifestDeriver::<DeletedManifest>::derive(
            ctx,
            derivation_ctx.blobstore(),
            bcs_id,
            parents
                .into_iter()
                .map(|root_mf_id| root_mf_id.deleted_manifest_id().clone())
                .collect(),
            changes,
        )
        .await?;
        Ok(RootDeletedManifestId(id))
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
        if let thrift::DerivedData::deleted_manifest(
            thrift::DerivedDataDeletedManifest::root_deleted_manifest_id(id),
        ) = data
        {
            DeletedManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::deleted_manifest(
            thrift::DerivedDataDeletedManifest::root_deleted_manifest_id(
                data.deleted_manifest_id().into_thrift(),
            ),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootDeletedManifestId);
