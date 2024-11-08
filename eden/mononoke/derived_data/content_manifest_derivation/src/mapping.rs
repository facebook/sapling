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
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use context::CoreContext;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if as thrift;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentManifestId;

use crate::derive::derive_content_manifest;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct RootContentManifestId(pub(crate) ContentManifestId);

impl RootContentManifestId {
    pub fn into_content_manifest_id(self) -> ContentManifestId {
        self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootContentManifestId {
    type Error = Error;

    fn try_from(value: BlobstoreBytes) -> Result<Self> {
        Ok(RootContentManifestId(ContentManifestId::from_bytes(
            value.into_bytes(),
        )?))
    }
}

impl TryFrom<BlobstoreGetData> for RootContentManifestId {
    type Error = Error;

    fn try_from(value: BlobstoreGetData) -> Result<Self> {
        value.into_bytes().try_into()
    }
}

impl From<RootContentManifestId> for BlobstoreBytes {
    fn from(value: RootContentManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(value.0.blake2().as_ref()))
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootContentManifestId>();
    format!("derived_root_contentmf.{key_prefix}{changeset_id}")
}

#[async_trait]
impl BonsaiDerivable for RootContentManifestId {
    const VARIANT: DerivableType = DerivableType::ContentManifests;

    type Dependencies = dependencies![];
    type PredecessorDependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        let content_manifest_id = derive_content_manifest(
            ctx,
            derivation_ctx,
            bonsai,
            parents
                .into_iter()
                .map(|id| id.into_content_manifest_id())
                .collect(),
        )
        .await?;
        Ok(RootContentManifestId(content_manifest_id))
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
        if let thrift::DerivedData::content_manifest(
            thrift::DerivedDataContentManifest::root_content_manifest_id(id),
        ) = data
        {
            Ok(RootContentManifestId(ContentManifestId::from_thrift(id)?))
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::content_manifest(
            thrift::DerivedDataContentManifest::root_content_manifest_id(
                data.into_content_manifest_id().into_thrift(),
            ),
        ))
    }
}
