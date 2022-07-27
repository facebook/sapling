/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BonsaiHgMapping;
use crate::BonsaiHgMappingEntry;
use crate::BonsaiOrHgChangesetIds;
use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use lock_ext::LockExt;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

type Cache = (
    HashMap<ChangesetId, HgChangesetId>,
    HashMap<HgChangesetId, ChangesetId>,
);

#[derive(Clone)]
pub struct MemWritesBonsaiHgMapping<T: BonsaiHgMapping + Clone + 'static> {
    inner: T,
    cache: Arc<Mutex<Cache>>,
    no_access_to_inner: Arc<AtomicBool>,
    readonly: Arc<AtomicBool>,
    save_noop_writes: Arc<AtomicBool>,
}

impl<T: BonsaiHgMapping + Clone + 'static> MemWritesBonsaiHgMapping<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cache: Default::default(),
            no_access_to_inner: Default::default(),
            readonly: Default::default(),
            save_noop_writes: Default::default(),
        }
    }

    pub fn set_readonly(&self, readonly: bool) {
        self.readonly.store(readonly, Ordering::Relaxed);
    }

    pub fn set_no_access_to_inner(&self, no_access_to_inner: bool) {
        self.no_access_to_inner
            .store(no_access_to_inner, Ordering::Relaxed);
    }

    pub fn set_save_noop_writes(&self, save_noop_writes: bool) {
        self.save_noop_writes
            .store(save_noop_writes, Ordering::Relaxed);
    }
}

impl<T: BonsaiHgMapping + Clone + 'static> MemWritesBonsaiHgMapping<T> {
    async fn get_from_cache_and_inner<I, O>(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<I>,
        get_cache: impl Fn(&Cache) -> &HashMap<I, O>,
        make_entry: impl Fn(I, O) -> BonsaiHgMappingEntry,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error>
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

                match cache.get(&i).copied() {
                    Some(o) => from_cache.push(make_entry(i, o)),
                    None => from_inner.push(i),
                };
            });
        }

        if !self.no_access_to_inner.load(Ordering::Relaxed) {
            let from_inner = self.inner.get(ctx, from_inner.into()).await?;
            from_cache.extend(from_inner);
        }
        Ok(from_cache)
    }
}

#[async_trait]
impl<T: BonsaiHgMapping + Clone + 'static> BonsaiHgMapping for MemWritesBonsaiHgMapping<T> {
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        if self.readonly.load(Ordering::Relaxed) {
            return Err(anyhow!(
                "cannot write to a readonly MemWritesBonsaiHgMapping"
            ));
        }

        let this = self.clone();

        let BonsaiHgMappingEntry { hg_cs_id, bcs_id } = entry;

        let entry = this.get_hg_from_bonsai(ctx, bcs_id).await?;
        if entry.is_some() && !self.save_noop_writes.load(Ordering::Relaxed) {
            Ok(false)
        } else {
            this.cache.with(|cache| {
                cache.0.insert(bcs_id, hg_cs_id);
                cache.1.insert(hg_cs_id, bcs_id);
            });
            Ok(true)
        }
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        cs_ids: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error> {
        match cs_ids {
            BonsaiOrHgChangesetIds::Bonsai(bcs_ids) => {
                self.get_from_cache_and_inner(
                    ctx,
                    bcs_ids,
                    |cache| &cache.0,
                    |bcs_id, hg_cs_id| BonsaiHgMappingEntry { bcs_id, hg_cs_id },
                )
                .await
            }
            BonsaiOrHgChangesetIds::Hg(hg_cs_ids) => {
                self.get_from_cache_and_inner(
                    ctx,
                    hg_cs_ids,
                    |cache| &cache.1,
                    |hg_cs_id, bcs_id| BonsaiHgMappingEntry { bcs_id, hg_cs_id },
                )
                .await
            }
        }
    }

    async fn get_hg_in_range(
        &self,
        _ctx: &CoreContext,
        _low: HgChangesetId,
        _high: HgChangesetId,
        _limit: usize,
    ) -> Result<Vec<HgChangesetId>, Error> {
        unimplemented!("This is not currently implemented in Gitimport")
    }
}
