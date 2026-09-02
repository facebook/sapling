/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivableUntopologically;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use futures::future;
use history_manifest::RootHistoryManifestDirectoryId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableUntopologicallyVariant;
use mononoke_types::HistoryManifestDirectoryId;

use crate::batch_v3::derive_blame_v3_in_batch;
use crate::derive_from_predecessor_v3::derive_blame_v3_from_predecessor;
use crate::derive_v3::derive_blame_v3;
use crate::mapping_v2::RootBlameV2;

const BLAME_V3_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootBlameV3 {
    pub(crate) csid: ChangesetId,
    pub(crate) root_manifest: RootHistoryManifestDirectoryId,
}

impl RootBlameV3 {
    pub fn root_manifest(&self) -> RootHistoryManifestDirectoryId {
        self.root_manifest
    }

    pub fn changeset_id(&self) -> ChangesetId {
        self.csid
    }
}

#[async_trait]
impl BonsaiDerivable for RootBlameV3 {
    const VARIANT: DerivableType = DerivableType::BlameV3;

    type Dependencies = dependencies![RootHistoryManifestDirectoryId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self, Error> {
        let csid = bonsai.get_changeset_id();
        let root_manifest = derivation_ctx
            .fetch_dependency::<RootHistoryManifestDirectoryId>(ctx, csid)
            .await?;
        derive_blame_v3(ctx, derivation_ctx, bonsai, root_manifest).await?;
        Ok(RootBlameV3 {
            csid,
            root_manifest,
        })
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>, Error> {
        derive_blame_v3_in_batch(ctx, derivation_ctx, bonsais).await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let mapping = derivation_ctx.commit_derived_data_mapping()?;
        let value = self
            .root_manifest
            .into_history_manifest_directory_id()
            .blake2()
            .as_ref()
            .to_vec();
        mapping
            .store_mapping(
                ctx,
                derivation_ctx.repo_id(),
                changeset_id,
                Self::VARIANT,
                BLAME_V3_VERSION,
                &value,
                derivation_ctx.xdb_shard_id(Self::VARIANT)?,
            )
            .await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let mapping = derivation_ctx.commit_derived_data_mapping()?;
        let value = mapping
            .fetch_mapping(
                ctx,
                derivation_ctx.repo_id(),
                changeset_id,
                Self::VARIANT,
                BLAME_V3_VERSION,
                derivation_ctx.xdb_shard_id(Self::VARIANT)?,
            )
            .await?;
        match value {
            Some(bytes) => {
                let hm_dir_id = HistoryManifestDirectoryId::from_bytes(Bytes::from(bytes))
                    .context("Failed to deserialize HistoryManifestDirectoryId from XDB mapping")?;
                Ok(Some(RootBlameV3 {
                    csid: changeset_id,
                    root_manifest: RootHistoryManifestDirectoryId::from(hm_dir_id),
                }))
            }
            None => Ok(None),
        }
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::blame_v3(thrift::DerivedDataBlame::root_blame_v3(blame)) = data
        {
            let hm_dir_id = match blame.history_manifest {
                thrift::DerivedDataHistoryManifest::root_history_manifest_directory_id(id) => {
                    HistoryManifestDirectoryId::from_thrift(id)
                }
                thrift::DerivedDataHistoryManifest::UnknownField(x) => Err(anyhow!(
                    "Can't convert {} from provided thrift::DerivedData, unknown field: {}",
                    Self::NAME,
                    x,
                )),
            }?;
            Ok(Self {
                csid: ChangesetId::from_thrift(blame.changeset_id)?,
                root_manifest: RootHistoryManifestDirectoryId::from(hm_dir_id),
            })
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME,
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::blame_v3(
            thrift::DerivedDataBlame::root_blame_v3(thrift::DerivedDataRootBlameV3 {
                changeset_id: data.csid.into_thrift(),
                history_manifest:
                    thrift::DerivedDataHistoryManifest::root_history_manifest_directory_id(
                        data.root_manifest
                            .into_history_manifest_directory_id()
                            .into_thrift(),
                    ),
            }),
        ))
    }
}

#[async_trait]
impl DerivableUntopologically for RootBlameV3 {
    const DERIVABLE_UNTOPOLOGICALLY_VARIANT: DerivableUntopologicallyVariant =
        DerivableUntopologicallyVariant::BlameV3;

    // Blame cannot be computed without ancestry, but blame v2 already holds
    // the same payload under a different key, so v3 can be transcoded from it.
    // This ties untopological v3 derivation to repos that have blame v2.
    type PredecessorDependencies = dependencies![RootBlameV2, RootHistoryManifestDirectoryId];

    async fn unsafe_derive_untopologically(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        let csid = bonsai.get_changeset_id();
        let (blame_v2, root_manifest) = future::try_join(
            derivation_ctx.fetch_dependency::<RootBlameV2>(ctx, csid),
            derivation_ctx.fetch_dependency::<RootHistoryManifestDirectoryId>(ctx, csid),
        )
        .await?;
        derive_blame_v3_from_predecessor(ctx, derivation_ctx.blobstore(), blame_v2, root_manifest)
            .await?;
        Ok(RootBlameV3 {
            csid,
            root_manifest,
        })
    }
}
