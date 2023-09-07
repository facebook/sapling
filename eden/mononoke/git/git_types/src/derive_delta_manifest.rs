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
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::delta_manifest::GitDeltaManifestId;
use crate::MappedGitCommitId;
use crate::TreeHandle;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct RootGitDeltaManifestId(GitDeltaManifestId);

impl RootGitDeltaManifestId {
    pub fn new(id: GitDeltaManifestId) -> Self {
        Self(id)
    }
}

impl TryFrom<BlobstoreBytes> for RootGitDeltaManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        GitDeltaManifestId::from_bytes(&blob_bytes.into_bytes()).map(RootGitDeltaManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootGitDeltaManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootGitDeltaManifestId> for BlobstoreBytes {
    fn from(root_gdm_id: RootGitDeltaManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_gdm_id.0.blake2().as_ref()))
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_git_delta_manifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootGitDeltaManifestId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for RootGitDeltaManifestId {
    const VARIANT: DerivableType = DerivableType::GitDeltaManifest;

    type Dependencies = dependencies![TreeHandle, MappedGitCommitId];

    async fn derive_single(
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        _bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self, Error> {
        todo!()
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
        if let thrift::DerivedData::git_delta_manifest(
            thrift::DerivedDataGitDeltaManifest::root_git_delta_manifest_id(id),
        ) = data
        {
            GitDeltaManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::git_delta_manifest(
            thrift::DerivedDataGitDeltaManifest::root_git_delta_manifest_id(data.0.into_thrift()),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootGitDeltaManifestId);
