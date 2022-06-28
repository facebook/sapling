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
use blobstore::BlobstoreBytes;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use metaconfig_types::BlameVersion;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use unodes::RootUnodeManifestId;

use crate::derive_v1::derive_blame_v1;

use derived_data_service_if::types as thrift;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlameRoot(ChangesetId);

impl BlameRoot {
    pub fn changeset_id(&self) -> &ChangesetId {
        &self.0
    }
}

impl From<ChangesetId> for BlameRoot {
    fn from(csid: ChangesetId) -> BlameRoot {
        BlameRoot(csid)
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_rootblame.v1.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<BlameRoot>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for BlameRoot {
    const NAME: &'static str = "blame";

    type Dependencies = dependencies![RootUnodeManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let csid = bonsai.get_changeset_id();
        let root_manifest = derivation_ctx
            .derive_dependency::<RootUnodeManifestId>(ctx, csid)
            .await?;
        if derivation_ctx.config().blame_version != BlameVersion::V1 {
            return Err(anyhow!(
                "programming error: incorrect blame version (expected V1)"
            ));
        }
        derive_blame_v1(ctx, derivation_ctx, bonsai, root_manifest).await?;
        Ok(BlameRoot(csid))
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx
            .blobstore()
            .put(ctx, key, BlobstoreBytes::empty())
            .await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        match derivation_ctx.blobstore().get(ctx, &key).await? {
            Some(_) => Ok(Some(BlameRoot(changeset_id))),
            None => Ok(None),
        }
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::blame(thrift::DerivedDataBlame::root_blame_v1(blame)) = data {
            ChangesetId::from_thrift(blame.blame_root_id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::blame(
            thrift::DerivedDataBlame::root_blame_v1(thrift::DerivedDataRootBlameV1 {
                blame_root_id: data.changeset_id().into_thrift(),
            }),
        ))
    }
}

impl_bonsai_derived_via_manager!(BlameRoot);
