/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::BlobstoreGetData;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;
use mononoke_types::typed_hash::DirectoryBranchClusterManifestId;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;

use crate::derive::derive_single;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootDirectoryBranchClusterManifestId(pub(crate) DirectoryBranchClusterManifestId);

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_dbcm.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootDirectoryBranchClusterManifestId>();
    format!("{root_prefix}{key_prefix}{changeset_id}")
}

impl TryFrom<BlobstoreBytes> for RootDirectoryBranchClusterManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        DirectoryBranchClusterManifestId::from_bytes(blob_bytes.into_bytes())
            .map(RootDirectoryBranchClusterManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootDirectoryBranchClusterManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootDirectoryBranchClusterManifestId> for BlobstoreBytes {
    fn from(root_mf_id: RootDirectoryBranchClusterManifestId) -> Self {
        BlobstoreBytes::from_bytes(root_mf_id.0.into_bytes())
    }
}

impl RootDirectoryBranchClusterManifestId {
    pub fn into_inner_id(self) -> DirectoryBranchClusterManifestId {
        self.0
    }
    pub fn inner_id(&self) -> &DirectoryBranchClusterManifestId {
        &self.0
    }
    pub fn directory_branch_cluster_manifest_id(&self) -> &DirectoryBranchClusterManifestId {
        &self.0
    }
    pub fn into_directory_branch_cluster_manifest_id(self) -> DirectoryBranchClusterManifestId {
        self.0
    }
}

#[async_trait]
impl BonsaiDerivable for RootDirectoryBranchClusterManifestId {
    const VARIANT: DerivableType = DerivableType::DirectoryBranchClusterManifest;

    type Dependencies = dependencies![RootSkeletonManifestV2Id];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai, parents, known).await
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
        if let thrift::DerivedData::directory_branch_cluster_manifest(thrift::DerivedDataDirectoryBranchClusterManifest::root_directory_branch_cluster_manifest_id(id)) = data {
            DirectoryBranchClusterManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME,
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::directory_branch_cluster_manifest(
            thrift::DerivedDataDirectoryBranchClusterManifest::root_directory_branch_cluster_manifest_id(data.0.into_thrift()),
        ))
    }
}
