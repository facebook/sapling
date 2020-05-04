/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds};
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{FutureExt as _, TryFutureExt},
};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::Future;
use lock_ext::LockExt;
use mercurial_types::{HgChangesetId, HgChangesetIdPrefix, HgChangesetIdsResolvedFromPrefix};
use mononoke_types::{ChangesetId, RepositoryId};
use std::cmp::Eq;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

type Cache = (
    HashMap<(RepositoryId, ChangesetId), HgChangesetId>,
    HashMap<(RepositoryId, HgChangesetId), ChangesetId>,
);

#[derive(Clone)]
pub struct MemWritesBonsaiHgMapping<T: BonsaiHgMapping + Clone + 'static> {
    inner: T,
    cache: Arc<Mutex<Cache>>,
}

impl<T: BonsaiHgMapping + Clone + 'static> MemWritesBonsaiHgMapping<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cache: Default::default(),
        }
    }
}

impl<T: BonsaiHgMapping + Clone + 'static> MemWritesBonsaiHgMapping<T> {
    fn get_from_cache_and_inner<I, O>(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<I>,
        get_cache: impl Fn(&Cache) -> &HashMap<(RepositoryId, I), O>,
        make_entry: impl Fn(RepositoryId, I, O) -> BonsaiHgMappingEntry,
    ) -> BoxFuture<Vec<BonsaiHgMappingEntry>, Error>
    where
        Vec<I>: Into<BonsaiOrHgChangesetIds>,
        I: Eq + Hash + Copy,
        O: Copy,
    {
        let mut from_cache = vec![];
        let mut from_inner = vec![];

        for i in cs_ids {
            self.cache.with(|cache| {
                let cache = get_cache(cache);

                match cache.get(&(repo_id, i)).copied() {
                    Some(o) => from_cache.push(make_entry(repo_id, i, o)),
                    None => from_inner.push(i),
                };
            });
        }

        self.inner
            .get(ctx, repo_id, from_inner.into())
            .map(move |from_inner| {
                from_cache.extend(from_inner);
                from_cache
            })
            .boxify()
    }
}

impl<T: BonsaiHgMapping + Clone + 'static> BonsaiHgMapping for MemWritesBonsaiHgMapping<T> {
    fn add(&self, ctx: CoreContext, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        let this = self.clone();

        let BonsaiHgMappingEntry {
            repo_id,
            hg_cs_id,
            bcs_id,
        } = entry;

        async move {
            let entry = this
                .get_hg_from_bonsai(ctx.clone(), repo_id, bcs_id)
                .compat()
                .await?;
            if entry.is_some() {
                Ok(false)
            } else {
                this.cache.with(|cache| {
                    cache.0.insert((repo_id, bcs_id), hg_cs_id);
                    cache.1.insert((repo_id, hg_cs_id), bcs_id);
                });
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
        cs_ids: BonsaiOrHgChangesetIds,
    ) -> BoxFuture<Vec<BonsaiHgMappingEntry>, Error> {
        match cs_ids {
            BonsaiOrHgChangesetIds::Bonsai(bcs_ids) => self.get_from_cache_and_inner(
                ctx,
                repo_id,
                bcs_ids,
                |cache| &cache.0,
                |repo_id, bcs_id, hg_cs_id| BonsaiHgMappingEntry {
                    repo_id,
                    bcs_id,
                    hg_cs_id,
                },
            ),
            BonsaiOrHgChangesetIds::Hg(hg_cs_ids) => self.get_from_cache_and_inner(
                ctx,
                repo_id,
                hg_cs_ids,
                |cache| &cache.1,
                |repo_id, hg_cs_id, bcs_id| BonsaiHgMappingEntry {
                    repo_id,
                    bcs_id,
                    hg_cs_id,
                },
            ),
        }
    }

    fn get_many_hg_by_prefix(
        &self,
        _ctx: CoreContext,
        _repo_id: RepositoryId,
        _cs_prefix: HgChangesetIdPrefix,
        _limit: usize,
    ) -> BoxFuture<HgChangesetIdsResolvedFromPrefix, Error> {
        unimplemented!("This is not currently implemented in Gitimport")
    }
}
