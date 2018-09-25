// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cachelib;
use changesets::{ChangesetEntry, Changesets};
use failure::{err_msg, Error};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::RepositoryId;
use mononoke_types::{ChangesetId, Generation};

use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering};

/// Trait that knows how to fetch DAG info about commits. Primary user is revsets
/// Concrete implementation may add more efficient caching logic to make request faster
pub trait ChangesetFetcher: Send + Sync {
    fn get_generation_number(&self, cs_id: ChangesetId) -> BoxFuture<Generation, Error>;

    fn get_parents(&self, cs_id: ChangesetId) -> BoxFuture<Vec<ChangesetId>, Error>;
}

/// Simplest ChangesetFetcher implementation which is just a wrapper around `Changesets` object
pub struct SimpleChangesetFetcher {
    changesets: Arc<Changesets>,
    repo_id: RepositoryId,
}

impl SimpleChangesetFetcher {
    pub fn new(changesets: Arc<Changesets>, repo_id: RepositoryId) -> Self {
        Self {
            changesets,
            repo_id,
        }
    }
}

impl ChangesetFetcher for SimpleChangesetFetcher {
    fn get_generation_number(&self, cs_id: ChangesetId) -> BoxFuture<Generation, Error> {
        self.changesets
            .get(self.repo_id.clone(), cs_id.clone())
            .and_then(move |maybe_cs| {
                maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id)))
            })
            .map(|cs| Generation::new(cs.gen))
            .boxify()
    }

    fn get_parents(&self, cs_id: ChangesetId) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.changesets
            .get(self.repo_id.clone(), cs_id.clone())
            .and_then(move |maybe_cs| {
                maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id)))
            })
            .map(|cs| cs.parents)
            .boxify()
    }
}

pub struct CachingChangesetFetcher {
    changesets: Arc<Changesets>,
    repo_id: RepositoryId,
    cache_pool: cachelib::LruCachePool,
    cache_misses: Arc<AtomicUsize>,
}

impl CachingChangesetFetcher {
    pub fn new(
        changesets: Arc<Changesets>,
        repo_id: RepositoryId,
        cache_pool: cachelib::LruCachePool,
    ) -> Self {
        Self {
            changesets,
            repo_id,
            cache_pool,
            cache_misses: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn get_changeset_entry(
        &self,
        cs_id: ChangesetId,
    ) -> impl Future<Item = ChangesetEntry, Error = Error> {
        let cache_key = format!("{}.{}", self.repo_id.prefix(), cs_id).to_string();

        cloned!(self.repo_id, self.cache_misses);
        cachelib::get_cached_or_fill(&self.cache_pool, cache_key, move || {
            cache_misses.fetch_add(1, Ordering::Relaxed);
            self.changesets.get(repo_id, cs_id)
        }).and_then(move |maybe_cs| maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id))))
    }
}

impl ChangesetFetcher for CachingChangesetFetcher {
    fn get_generation_number(&self, cs_id: ChangesetId) -> BoxFuture<Generation, Error> {
        self.get_changeset_entry(cs_id.clone())
            .map(|cs| Generation::new(cs.gen))
            .boxify()
    }

    fn get_parents(&self, cs_id: ChangesetId) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.get_changeset_entry(cs_id.clone())
            .map(|cs| cs.parents)
            .boxify()
    }
}
