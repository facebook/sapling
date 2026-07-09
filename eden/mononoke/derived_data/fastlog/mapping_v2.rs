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
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use history_manifest::RootHistoryManifestDirectoryId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::HistoryManifestDirectoryId;

use crate::derive_v2::derive_fastlog_v2;

const FASTLOG_V2_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootFastlogV2 {
    pub(crate) csid: ChangesetId,
    pub(crate) root_manifest: RootHistoryManifestDirectoryId,
}

impl RootFastlogV2 {
    pub fn root_manifest(&self) -> RootHistoryManifestDirectoryId {
        self.root_manifest
    }

    pub fn changeset_id(&self) -> ChangesetId {
        self.csid
    }
}

#[async_trait]
impl BonsaiDerivable for RootFastlogV2 {
    const VARIANT: DerivableType = DerivableType::FastlogV2;

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
        derive_fastlog_v2(ctx, derivation_ctx, bonsai, root_manifest).await?;
        Ok(RootFastlogV2 {
            csid,
            root_manifest,
        })
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
                FASTLOG_V2_VERSION,
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
                FASTLOG_V2_VERSION,
                derivation_ctx.xdb_shard_id(Self::VARIANT)?,
            )
            .await?;
        match value {
            Some(bytes) => {
                let hm_dir_id = HistoryManifestDirectoryId::from_bytes(Bytes::from(bytes))
                    .context("Failed to deserialize HistoryManifestDirectoryId from XDB mapping")?;
                Ok(Some(RootFastlogV2 {
                    csid: changeset_id,
                    root_manifest: RootHistoryManifestDirectoryId::from(hm_dir_id),
                }))
            }
            None => Ok(None),
        }
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::fastlog_v2(thrift::DerivedDataFastlog::root_fastlog_v2(
            fastlog,
        )) = data
        {
            let hm_dir_id = match fastlog.history_manifest {
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
                csid: ChangesetId::from_thrift(fastlog.changeset_id)?,
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
        Ok(thrift::DerivedData::fastlog_v2(
            thrift::DerivedDataFastlog::root_fastlog_v2(thrift::DerivedDataRootFastlogV2 {
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
