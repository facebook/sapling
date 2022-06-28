/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use context::CoreContext;
use futures::future;
use futures::stream::BoxStream;
use lock_ext::LockExt;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub struct MemWritesChangesets<T: Changesets + Clone + 'static> {
    repo_id: RepositoryId,
    inner: T,
    cache: Arc<Mutex<HashMap<ChangesetId, ChangesetEntry>>>,
}

impl<T: Changesets + Clone + 'static> MemWritesChangesets<T> {
    pub fn new(inner: T) -> Self {
        Self {
            repo_id: inner.repo_id(),
            inner,
            cache: Default::default(),
        }
    }
}

#[async_trait]
impl<T: Changesets + Clone + 'static> Changesets for MemWritesChangesets<T> {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: CoreContext, ci: ChangesetInsert) -> Result<bool, Error> {
        let ChangesetInsert { cs_id, parents } = ci;

        let cs = self.get(ctx.clone(), cs_id);
        let parent_css = self.get_many(ctx.clone(), parents.clone());
        let (cs, parent_css) = future::try_join(cs, parent_css).await?;

        if cs.is_some() {
            Ok(false)
        } else {
            let gen = parent_css.into_iter().map(|p| p.gen).max().unwrap_or(0);

            let entry = ChangesetEntry {
                repo_id: self.repo_id,
                cs_id,
                parents,
                gen,
            };

            self.cache.with(|cache| cache.insert(cs_id, entry));

            Ok(true)
        }
    }

    async fn get(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        match self.cache.with(|cache| cache.get(&cs_id).cloned()) {
            Some(entry) => Ok(Some(entry)),
            None => self.inner.get(ctx, cs_id).await,
        }
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        let mut from_cache = vec![];
        let mut from_inner = vec![];

        for cs_id in cs_ids {
            match self.cache.with(|cache| cache.get(&cs_id).cloned()) {
                Some(entry) => from_cache.push(entry),
                None => from_inner.push(cs_id),
            };
        }

        let from_inner = self.inner.get_many(ctx, from_inner).await?;
        from_cache.extend(from_inner);
        Ok(from_cache)
    }

    async fn get_many_by_prefix(
        &self,
        _ctx: CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
        unimplemented!("This is not currently implemented in Gitimport")
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.inner.prime_cache(ctx, changesets)
    }

    async fn enumeration_bounds(
        &self,
        ctx: &CoreContext,
        read_from_master: bool,
        known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>, Error> {
        self.inner
            .enumeration_bounds(ctx, read_from_master, known_heads)
            .await
    }

    fn list_enumeration_range(
        &self,
        ctx: &CoreContext,
        min_id: u64,
        max_id: u64,
        sort_and_limit: Option<(SortOrder, u64)>,
        read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64), Error>> {
        self.inner
            .list_enumeration_range(ctx, min_id, max_id, sort_and_limit, read_from_master)
    }
}
