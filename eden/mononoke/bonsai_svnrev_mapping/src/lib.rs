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
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Svnrev;
use slog::warn;

pub use crate::caching::CachingBonsaiSvnrevMapping;
pub use crate::sql::bulk_import_svnrevs;
pub use crate::sql::AddSvnrevsErrorKind;
pub use crate::sql::SqlBonsaiSvnrevMapping;
pub use crate::sql::SqlBonsaiSvnrevMappingBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiSvnrevMappingEntry {
    pub bcs_id: ChangesetId,
    pub svnrev: Svnrev,
}

impl BonsaiSvnrevMappingEntry {
    pub fn new(bcs_id: ChangesetId, svnrev: Svnrev) -> Self {
        BonsaiSvnrevMappingEntry { bcs_id, svnrev }
    }
}

pub enum BonsaisOrSvnrevs {
    Bonsai(Vec<ChangesetId>),
    Svnrev(Vec<Svnrev>),
}

impl BonsaisOrSvnrevs {
    pub fn is_empty(&self) -> bool {
        match self {
            BonsaisOrSvnrevs::Bonsai(v) => v.is_empty(),
            BonsaisOrSvnrevs::Svnrev(v) => v.is_empty(),
        }
    }
}

impl From<ChangesetId> for BonsaisOrSvnrevs {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaisOrSvnrevs::Bonsai(vec![cs_id])
    }
}

impl From<Vec<ChangesetId>> for BonsaisOrSvnrevs {
    fn from(cs_ids: Vec<ChangesetId>) -> Self {
        BonsaisOrSvnrevs::Bonsai(cs_ids)
    }
}

impl From<Svnrev> for BonsaisOrSvnrevs {
    fn from(rev: Svnrev) -> Self {
        BonsaisOrSvnrevs::Svnrev(vec![rev])
    }
}

impl From<Vec<Svnrev>> for BonsaisOrSvnrevs {
    fn from(revs: Vec<Svnrev>) -> Self {
        BonsaisOrSvnrevs::Svnrev(revs)
    }
}

#[facet::facet]
#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait BonsaiSvnrevMapping: Send + Sync {
    fn repo_id(&self) -> RepositoryId;

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiSvnrevMappingEntry],
    ) -> Result<(), Error>;

    async fn get(
        &self,
        ctx: &CoreContext,
        field: BonsaisOrSvnrevs,
    ) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error>;

    async fn get_svnrev_from_bonsai(
        &self,
        ctx: &CoreContext,
        bcs_id: ChangesetId,
    ) -> Result<Option<Svnrev>, Error> {
        let result = self
            .get(ctx, BonsaisOrSvnrevs::Bonsai(vec![bcs_id]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.svnrev))
    }

    async fn get_bonsai_from_svnrev(
        &self,
        ctx: &CoreContext,
        svnrev: Svnrev,
    ) -> Result<Option<ChangesetId>, Error> {
        let result = self
            .get(ctx, BonsaisOrSvnrevs::Svnrev(vec![svnrev]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.bcs_id))
    }

    async fn bulk_import_from_bonsai(
        &self,
        ctx: &CoreContext,
        changesets: &[BonsaiChangeset],
    ) -> anyhow::Result<()> {
        let mut entries = vec![];
        for bcs in changesets.iter() {
            match Svnrev::from_bcs(bcs) {
                Ok(svnrev) => {
                    let entry = BonsaiSvnrevMappingEntry::new(bcs.get_changeset_id(), svnrev);
                    entries.push(entry);
                }
                Err(e) => {
                    warn!(ctx.logger(), "Couldn't fetch svnrev from commit: {:?}", e);
                }
            }
        }
        self.bulk_import(ctx, &entries).await?;
        Ok(())
    }
}
