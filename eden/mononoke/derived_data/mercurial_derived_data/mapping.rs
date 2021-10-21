/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Error, Result};
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::{dependencies, BonsaiDerivable, DerivationContext};
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::collections::HashMap;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MappedHgChangesetId(pub HgChangesetId);

#[derive(Debug, Clone)]
pub struct HgChangesetDeriveOptions {
    pub set_committer_field: bool,
}

#[async_trait]
impl BonsaiDerivable for MappedHgChangesetId {
    const NAME: &'static str = "hgchangesets";

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        if bonsai.is_snapshot() {
            bail!("Can't derive Hg changeset for snapshot")
        }
        crate::derive_hg_changeset::derive_from_parents(
            ctx,
            derivation_ctx.blobstore(),
            bonsai,
            parents,
            &HgChangesetDeriveOptions {
                set_committer_field: derivation_ctx.config().hg_set_committer_extra,
            },
        )
        .await
    }


    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        derivation_ctx
            .bonsai_hg_mapping()?
            .add(
                ctx,
                BonsaiHgMappingEntry {
                    repo_id: derivation_ctx.repo_id(),
                    hg_cs_id: self.0,
                    bcs_id: changeset_id,
                },
            )
            .await?;
        Ok(())
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        Ok(Self::fetch_batch(ctx, derivation_ctx, &[changeset_id])
            .await?
            .into_iter()
            .next()
            .map(|(_, hg_id)| hg_id))
    }

    async fn fetch_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Self>> {
        Ok(derivation_ctx
            .bonsai_hg_mapping()?
            .get(ctx, derivation_ctx.repo_id(), changeset_ids.to_vec().into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, MappedHgChangesetId(entry.hg_cs_id)))
            .collect())
    }
}

impl_bonsai_derived_via_manager!(MappedHgChangesetId);
