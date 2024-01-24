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
use blobstore::BlobstoreGetData;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BssmV3DirectoryId;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;
use skeleton_manifest::RootSkeletonManifestId;

use crate::derive::derive_single;
use crate::derive_from_predecessor::derive_from_predecessor;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootBssmV3DirectoryId(pub(crate) BssmV3DirectoryId);

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_bssm3.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootBssmV3DirectoryId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootBssmV3DirectoryId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        BssmV3DirectoryId::from_bytes(&blob_bytes.into_bytes()).map(RootBssmV3DirectoryId)
    }
}

impl TryFrom<BlobstoreGetData> for RootBssmV3DirectoryId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootBssmV3DirectoryId> for BlobstoreBytes {
    fn from(root_mf_id: RootBssmV3DirectoryId) -> Self {
        BlobstoreBytes::from_bytes(root_mf_id.0.into_bytes())
    }
}

impl RootBssmV3DirectoryId {
    pub fn into_inner_id(self) -> BssmV3DirectoryId {
        self.0
    }
    pub fn inner_id(&self) -> &BssmV3DirectoryId {
        &self.0
    }
}

#[async_trait]
impl BonsaiDerivable for RootBssmV3DirectoryId {
    const VARIANT: DerivableType = DerivableType::BssmV3;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        let parent_skeleton_manifests = stream::iter(bonsai.parents())
            .map(|parent| derivation_ctx.derive_dependency::<RootSkeletonManifestId>(ctx, parent))
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?;

        derive_single(
            ctx,
            derivation_ctx,
            bonsai,
            parents,
            parent_skeleton_manifests,
        )
        .await
    }

    async fn derive_from_predecessor(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        let csid = bonsai.get_changeset_id();
        let skeleton_manifest = derivation_ctx
            .derive_dependency::<RootSkeletonManifestId>(ctx, csid)
            .await?;
        derive_from_predecessor(ctx, derivation_ctx, skeleton_manifest).await
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
        derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::bssm_v3(thrift::DerivedDataBssmV3::root_bssm_v3_directory_id(
            id,
        )) = data
        {
            BssmV3DirectoryId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::bssm_v3(
            thrift::DerivedDataBssmV3::root_bssm_v3_directory_id(data.0.into_thrift()),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootBssmV3DirectoryId);
