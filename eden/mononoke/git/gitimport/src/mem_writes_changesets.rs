/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use changesets::{ChangesetEntry, ChangesetInsert, Changesets, SqlChangesets};
use context::CoreContext;
use futures::future;
use lock_ext::LockExt;
use mononoke_types::{
    ChangesetId, ChangesetIdPrefix, ChangesetIdsResolvedFromPrefix, RepositoryId,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct MemWritesChangesets<T: Changesets + Clone + 'static> {
    inner: T,
    cache: Arc<Mutex<HashMap<(RepositoryId, ChangesetId), ChangesetEntry>>>,
}

impl<T: Changesets + Clone + 'static> MemWritesChangesets<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cache: Default::default(),
        }
    }
}

#[async_trait]
impl<T: Changesets + Clone + 'static> Changesets for MemWritesChangesets<T> {
    async fn add(&self, ctx: CoreContext, ci: ChangesetInsert) -> Result<bool, Error> {
        let ChangesetInsert {
            repo_id,
            cs_id,
            parents,
        } = ci;

        let cs = self.get(ctx.clone(), repo_id, cs_id);
        let parent_css = self.get_many(ctx.clone(), repo_id, parents.clone());
        let (cs, parent_css) = future::try_join(cs, parent_css).await?;

        if cs.is_some() {
            Ok(false)
        } else {
            let gen = parent_css.into_iter().map(|p| p.gen).max().unwrap_or(0);

            let entry = ChangesetEntry {
                repo_id,
                cs_id,
                parents,
                gen,
            };

            self.cache
                .with(|cache| cache.insert((repo_id, cs_id), entry));

            Ok(true)
        }
    }

    async fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        match self
            .cache
            .with(|cache| cache.get(&(repo_id, cs_id)).cloned())
        {
            Some(entry) => Ok(Some(entry)),
            None => self.inner.get(ctx, repo_id, cs_id).await,
        }
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        let mut from_cache = vec![];
        let mut from_inner = vec![];

        for cs_id in cs_ids {
            match self
                .cache
                .with(|cache| cache.get(&(repo_id, cs_id)).cloned())
            {
                Some(entry) => from_cache.push(entry),
                None => from_inner.push(cs_id),
            };
        }

        let from_inner = self.inner.get_many(ctx, repo_id, from_inner).await?;
        from_cache.extend(from_inner);
        Ok(from_cache)
    }

    async fn get_many_by_prefix(
        &self,
        _ctx: CoreContext,
        _repo_id: RepositoryId,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
        unimplemented!("This is not currently implemented in Gitimport")
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.inner.prime_cache(ctx, changesets)
    }

    fn get_sql_changesets(&self) -> &SqlChangesets {
        self.inner.get_sql_changesets()
    }
}
