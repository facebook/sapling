/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use lock_ext::LockExt;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

use crate::BonsaiHgMapping;
use crate::BonsaiHgMappingEntry;
use crate::BonsaiOrHgChangesetIds;

type Cache = (
    HashMap<ChangesetId, HgChangesetId>,
    HashMap<HgChangesetId, ChangesetId>,
);

pub struct MemWritesBonsaiHgMapping {
    inner: Arc<dyn BonsaiHgMapping>,
    cache: Arc<Mutex<Cache>>,
    no_access_to_inner: Arc<AtomicBool>,
    readonly: Arc<AtomicBool>,
    save_noop_writes: Arc<AtomicBool>,
}

impl MemWritesBonsaiHgMapping {
    pub fn new(inner: Arc<dyn BonsaiHgMapping>) -> Self {
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

impl MemWritesBonsaiHgMapping {
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
impl BonsaiHgMapping for MemWritesBonsaiHgMapping {
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        self.bulk_add(ctx, &[entry]).await.map(|rows| rows >= 1)
    }

    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiHgMappingEntry],
    ) -> Result<u64, Error> {
        if self.readonly.load(Ordering::Relaxed) {
            return Err(anyhow!(
                "cannot write to a readonly MemWritesBonsaiHgMapping"
            ));
        }

        let save_noop_writes = self.save_noop_writes.load(Ordering::Relaxed);

        let to_insert = if save_noop_writes {
            entries.to_vec()
        } else {
            let bcs_ids: Vec<_> = entries.iter().map(|e| e.bcs_id).collect();
            let existing = self
                .get(ctx, BonsaiOrHgChangesetIds::Bonsai(bcs_ids))
                .await?;
            let existing_bcs_ids: HashSet<_> = existing.iter().map(|e| e.bcs_id).collect();
            entries
                .iter()
                .filter(|e| !existing_bcs_ids.contains(&e.bcs_id))
                .cloned()
                .collect()
        };

        let added = to_insert.len() as u64;
        self.cache.with(|cache| {
            for entry in to_insert {
                cache.0.insert(entry.bcs_id, entry.hg_cs_id);
                cache.1.insert(entry.hg_cs_id, entry.bcs_id);
            }
        });
        Ok(added)
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
