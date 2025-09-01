/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;

use crate::MappedGitCommitId;
use crate::delta_manifest_v3::GitDeltaManifestV3Id;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RootGitDeltaManifestV3Id(GitDeltaManifestV3Id);

impl RootGitDeltaManifestV3Id {
    pub fn manifest_id(&self) -> &GitDeltaManifestV3Id {
        &self.0
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_gdm3.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootGitDeltaManifestV3Id>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootGitDeltaManifestV3Id {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        GitDeltaManifestV3Id::from_bytes(blob_bytes.into_bytes()).map(RootGitDeltaManifestV3Id)
    }
}

impl TryFrom<BlobstoreGetData> for RootGitDeltaManifestV3Id {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootGitDeltaManifestV3Id> for BlobstoreBytes {
    fn from(root_mf_id: RootGitDeltaManifestV3Id) -> Self {
        BlobstoreBytes::from_bytes(root_mf_id.0.into_bytes())
    }
}

async fn derive_single(
    _ctx: &CoreContext,
    _derivation_ctx: &DerivationContext,
    _bonsai: BonsaiChangeset,
) -> Result<RootGitDeltaManifestV3Id> {
    unimplemented!("derivation is not implemented for GitDeltaManifestsV3");
}

#[async_trait]
impl BonsaiDerivable for RootGitDeltaManifestV3Id {
    const VARIANT: DerivableType = DerivableType::GitDeltaManifestsV3;

    type Dependencies = dependencies![MappedGitCommitId];
    type PredecessorDependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai).await
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        let ctx = Arc::new(ctx.clone());
        let derivation_ctx = Arc::new(derivation_ctx.clone());
        let output = stream::iter(bonsais)
            .map(Ok)
            .map_ok(|bonsai| {
                cloned!(ctx, derivation_ctx);
                async move {
                    let output = mononoke::spawn_task(async move {
                        let bonsai_id = bonsai.get_changeset_id();
                        let gdm_v3 = derive_single(&ctx, &derivation_ctx, bonsai).await?;
                        anyhow::Ok((bonsai_id, gdm_v3))
                    })
                    .await??;
                    anyhow::Ok(output)
                }
            })
            .try_buffer_unordered(100)
            .try_collect::<Vec<_>>()
            .await?;
        Ok(output.into_iter().collect())
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
        if let thrift::DerivedData::git_delta_manifest_v3(
            thrift::DerivedDataGitDeltaManifestV3::root_git_delta_manifest_v3_id(id),
        ) = data
        {
            GitDeltaManifestV3Id::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::git_delta_manifest_v3(
            thrift::DerivedDataGitDeltaManifestV3::root_git_delta_manifest_v3_id(
                data.0.into_thrift(),
            ),
        ))
    }
}
