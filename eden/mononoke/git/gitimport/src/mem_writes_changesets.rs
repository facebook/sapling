/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use changesets::{ChangesetEntry, ChangesetInsert, Changesets, SqlChangesets};
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{self, FutureExt as _, TryFutureExt},
};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::{future as future_old, Future};
use lock_ext::LockExt;
use mononoke_types::{
    ChangesetId, ChangesetIdPrefix, ChangesetIdsResolvedFromPrefix, RepositoryId,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

impl<T: Changesets + Clone + 'static> Changesets for MemWritesChangesets<T> {
    fn add(&self, ctx: CoreContext, ci: ChangesetInsert) -> BoxFuture<bool, Error> {
        let this = self.clone();

        let ChangesetInsert {
            repo_id,
            cs_id,
            parents,
        } = ci;

        async move {
            let cs = this.get(ctx.clone(), repo_id, cs_id).compat();
            let parent_css = this
                .get_many(ctx.clone(), repo_id, parents.clone())
                .compat();
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

                this.cache
                    .with(|cache| cache.insert((repo_id, cs_id), entry));

                Ok(true)
            }
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        match self
            .cache
            .with(|cache| cache.get(&(repo_id, cs_id)).cloned())
        {
            Some(entry) => future_old::ok(Some(entry)).boxify(),
            None => self.inner.get(ctx, repo_id, cs_id).boxify(),
        }
    }

    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
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

        self.inner
            .get_many(ctx, repo_id, from_inner)
            .map(move |from_inner| {
                from_cache.extend(from_inner);
                from_cache
            })
            .boxify()
    }

    fn get_many_by_prefix(
        &self,
        _ctx: CoreContext,
        _repo_id: RepositoryId,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> BoxFuture<ChangesetIdsResolvedFromPrefix, Error> {
        unimplemented!("This is not currently implemented in Gitimport")
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.inner.prime_cache(ctx, changesets)
    }

    fn get_sql_changesets(&self) -> &SqlChangesets {
        self.inner.get_sql_changesets()
    }
}
