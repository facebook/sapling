/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod caching;
mod sql;

use anyhow::Error;
use async_trait::async_trait;
use auto_impl::auto_impl;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;

pub use crate::caching::CachingBonsaiGlobalrevMapping;
pub use crate::sql::add_globalrevs;
pub use crate::sql::bulk_import_globalrevs;
pub use crate::sql::AddGlobalrevsErrorKind;
pub use crate::sql::SqlBonsaiGlobalrevMapping;
pub use crate::sql::SqlBonsaiGlobalrevMappingBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGlobalrevMappingEntry {
    pub bcs_id: ChangesetId,
    pub globalrev: Globalrev,
}

impl BonsaiGlobalrevMappingEntry {
    pub fn new(bcs_id: ChangesetId, globalrev: Globalrev) -> Self {
        BonsaiGlobalrevMappingEntry { bcs_id, globalrev }
    }
}

pub enum BonsaisOrGlobalrevs {
    Bonsai(Vec<ChangesetId>),
    Globalrev(Vec<Globalrev>),
}

impl BonsaisOrGlobalrevs {
    pub fn is_empty(&self) -> bool {
        match self {
            BonsaisOrGlobalrevs::Bonsai(v) => v.is_empty(),
            BonsaisOrGlobalrevs::Globalrev(v) => v.is_empty(),
        }
    }
}

impl From<ChangesetId> for BonsaisOrGlobalrevs {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaisOrGlobalrevs::Bonsai(vec![cs_id])
    }
}

impl From<Vec<ChangesetId>> for BonsaisOrGlobalrevs {
    fn from(cs_ids: Vec<ChangesetId>) -> Self {
        BonsaisOrGlobalrevs::Bonsai(cs_ids)
    }
}

impl From<Globalrev> for BonsaisOrGlobalrevs {
    fn from(rev: Globalrev) -> Self {
        BonsaisOrGlobalrevs::Globalrev(vec![rev])
    }
}

impl From<Vec<Globalrev>> for BonsaisOrGlobalrevs {
    fn from(revs: Vec<Globalrev>) -> Self {
        BonsaisOrGlobalrevs::Globalrev(revs)
    }
}

#[facet::facet]
#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait BonsaiGlobalrevMapping: Send + Sync {
    fn repo_id(&self) -> RepositoryId;

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGlobalrevMappingEntry],
    ) -> Result<(), Error>;

    async fn get(
        &self,
        ctx: &CoreContext,
        field: BonsaisOrGlobalrevs,
    ) -> Result<Vec<BonsaiGlobalrevMappingEntry>, Error>;

    async fn get_globalrev_from_bonsai(
        &self,
        ctx: &CoreContext,
        bcs_id: ChangesetId,
    ) -> Result<Option<Globalrev>, Error> {
        let result = self
            .get(ctx, BonsaisOrGlobalrevs::Bonsai(vec![bcs_id]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.globalrev))
    }

    async fn get_bonsai_from_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<ChangesetId>, Error> {
        let result = self
            .get(ctx, BonsaisOrGlobalrevs::Globalrev(vec![globalrev]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.bcs_id))
    }

    async fn get_closest_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error>;

    /// Read the most recent Globalrev. This produces the freshest data possible, and is meant to
    /// be used for Globalrev assignment.
    async fn get_max(&self, ctx: &CoreContext) -> Result<Option<Globalrev>, Error>;
}
