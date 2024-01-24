/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(impl_trait_in_assoc_type)]

mod caching;
mod sql;

use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;

pub use crate::caching::BonsaiGlobalrevMappingCacheEntry;
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

/// Internally, store a cache friendly representation of the data.
/// Through the public interface, only show the outcome of the query that a client would be
/// interested in (globalrev gaps result in no entry as opposed to an entry with a `globalrev` and a
/// None `bcs_id`)
#[derive(Debug, Eq, PartialEq)]
pub struct BonsaiGlobalrevMappingEntries {
    // These are the mappings used for caching, including negative lookup: `bcs_id` is an `Option`
    // or in other words, we allow for caching of globalrev gaps
    cached_data: Vec<BonsaiGlobalrevMappingCacheEntry>,
}

impl From<BonsaiGlobalrevMappingEntries> for Vec<BonsaiGlobalrevMappingEntry> {
    fn from(value: BonsaiGlobalrevMappingEntries) -> Self {
        value.into_iter().collect()
    }
}

impl IntoIterator for BonsaiGlobalrevMappingEntries {
    type Item = BonsaiGlobalrevMappingEntry;
    type IntoIter = impl Iterator<Item = BonsaiGlobalrevMappingEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.cached_data.into_iter().filter_map(|val| {
            val.bcs_id.map(|bcs_id| BonsaiGlobalrevMappingEntry {
                bcs_id,
                globalrev: val.globalrev,
            })
        })
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
    ) -> Result<BonsaiGlobalrevMappingEntries, Error>;

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

    async fn get_max_custom_repo(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
    ) -> Result<Option<Globalrev>, Error>;
}
