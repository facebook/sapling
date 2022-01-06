/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod caching;
mod sql;

use std::sync::Arc;

use abomonation_derive::Abomonation;
use anyhow::Error;
use async_trait::async_trait;
use auto_impl::auto_impl;
use context::CoreContext;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId, Svnrev};
use slog::warn;

pub use crate::caching::CachingBonsaiSvnrevMapping;
pub use crate::sql::{bulk_import_svnrevs, AddSvnrevsErrorKind, SqlBonsaiSvnrevMapping};

#[derive(Abomonation, Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiSvnrevMappingEntry {
    pub repo_id: RepositoryId,
    pub bcs_id: ChangesetId,
    pub svnrev: Svnrev,
}

impl BonsaiSvnrevMappingEntry {
    pub fn new(repo_id: RepositoryId, bcs_id: ChangesetId, svnrev: Svnrev) -> Self {
        BonsaiSvnrevMappingEntry {
            repo_id,
            bcs_id,
            svnrev,
        }
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

#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait BonsaiSvnrevMapping: Send + Sync {
    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiSvnrevMappingEntry],
    ) -> Result<(), Error>;

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        field: BonsaisOrSvnrevs,
    ) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error>;

    async fn get_svnrev_from_bonsai(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> Result<Option<Svnrev>, Error> {
        let result = self
            .get(ctx, repo_id, BonsaisOrSvnrevs::Bonsai(vec![bcs_id]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.svnrev))
    }

    async fn get_bonsai_from_svnrev(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        svnrev: Svnrev,
    ) -> Result<Option<ChangesetId>, Error> {
        let result = self
            .get(ctx, repo_id, BonsaisOrSvnrevs::Svnrev(vec![svnrev]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.bcs_id))
    }

    async fn bulk_import_from_bonsai(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        changesets: &[BonsaiChangeset],
    ) -> anyhow::Result<()> {
        let mut entries = vec![];
        for bcs in changesets.into_iter() {
            match Svnrev::from_bcs(bcs) {
                Ok(svnrev) => {
                    let entry =
                        BonsaiSvnrevMappingEntry::new(repo_id, bcs.get_changeset_id(), svnrev);
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

#[facet::facet]
#[derive(Clone)]
pub struct RepoBonsaiSvnrevMapping {
    inner: Arc<dyn BonsaiSvnrevMapping + Send + Sync + 'static>,
    repo_id: RepositoryId,
}

impl RepoBonsaiSvnrevMapping {
    pub fn new(
        repo_id: RepositoryId,
        inner: Arc<dyn BonsaiSvnrevMapping + Send + Sync + 'static>,
    ) -> RepoBonsaiSvnrevMapping {
        RepoBonsaiSvnrevMapping { inner, repo_id }
    }

    pub async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiSvnrevMappingEntry],
    ) -> Result<(), Error> {
        self.inner.bulk_import(ctx, entries).await
    }

    pub async fn get(
        &self,
        ctx: &CoreContext,
        field: BonsaisOrSvnrevs,
    ) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error> {
        self.inner.get(ctx, self.repo_id, field).await
    }

    pub async fn get_svnrev_from_bonsai(
        &self,
        ctx: &CoreContext,
        bcs_id: ChangesetId,
    ) -> Result<Option<Svnrev>, Error> {
        self.inner
            .get_svnrev_from_bonsai(ctx, self.repo_id, bcs_id)
            .await
    }

    pub async fn get_bonsai_from_svnrev(
        &self,
        ctx: &CoreContext,
        svnrev: Svnrev,
    ) -> Result<Option<ChangesetId>, Error> {
        self.inner
            .get_bonsai_from_svnrev(ctx, self.repo_id, svnrev)
            .await
    }

    pub async fn bulk_import_from_bonsai(
        &self,
        ctx: &CoreContext,
        changesets: &[BonsaiChangeset],
    ) -> anyhow::Result<()> {
        self.inner
            .bulk_import_from_bonsai(ctx, self.repo_id, changesets)
            .await
    }
}
