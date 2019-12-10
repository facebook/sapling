/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Main function is `new_benchmark_repo` which creates `BlobRepo` which delay applied
//! to all underlying stores, but which all the caching enabled.
use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMapping;
use bonsai_hg_mapping::{
    BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds, CachingBonsaiHgMapping,
    SqlBonsaiHgMapping,
};
use cacheblob::{dummy::DummyLease, new_cachelib_blobstore};
use changesets::{CachingChangesets, ChangesetEntry, ChangesetInsert, Changesets, SqlChangesets};
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use fbinit::FacebookInit;
use filenodes::{CachingFilenodes, FilenodeInfo, Filenodes};
use filestore::FilestoreConfig;
use futures::{future, Future};
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use memblob::EagerMemblob;
use mercurial_types::HgFileNodeId;
use mononoke_types::{BlobstoreBytes, ChangesetId, RepoPath, RepositoryId};
use rand::Rng;
use rand_distr::Distribution;
use repo_blobstore::RepoBlobstoreArgs;
use scuba_ext::ScubaSampleBuilder;
use sql_ext::SqlConstructors;
use sqlfilenodes::SqlFilenodes;
use std::{sync::Arc, time::Duration};

pub type Normal = rand_distr::Normal<f64>;

pub struct DelaySettings {
    pub blobstore_put_dist: Normal,
    pub blobstore_get_dist: Normal,
    pub db_put_dist: Normal,
    pub db_get_dist: Normal,
}

impl Default for DelaySettings {
    fn default() -> Self {
        Self {
            blobstore_put_dist: Normal::new(0.1, 0.05).expect("Normal::new failed"),
            blobstore_get_dist: Normal::new(0.05, 0.025).expect("Normal::new failed"),
            db_put_dist: Normal::new(0.02, 0.01).expect("Normal::new failed"),
            db_get_dist: Normal::new(0.02, 0.01).expect("Normal::new failed"),
        }
    }
}

pub fn new_benchmark_repo(fb: FacebookInit, settings: DelaySettings) -> Result<BlobRepo> {
    let blobstore: Arc<dyn Blobstore> = {
        let delayed: Arc<dyn Blobstore> = Arc::new(DelayedBlobstore::new(
            EagerMemblob::new(),
            settings.blobstore_get_dist,
            settings.blobstore_put_dist,
        ));
        Arc::new(new_cachelib_blobstore(
            delayed,
            Arc::new(cachelib::get_pool("blobstore-blobs").ok_or(Error::msg("no cache pool"))?),
            Arc::new(cachelib::get_pool("blobstore-presence").ok_or(Error::msg("no cache pool"))?),
        ))
    };

    let filenodes = {
        let filenodes: Arc<dyn Filenodes> = Arc::new(DelayedFilenodes::new(
            SqlFilenodes::with_sqlite_in_memory()?,
            settings.db_get_dist,
            settings.db_put_dist,
        ));
        Arc::new(CachingFilenodes::new(
            fb,
            filenodes,
            cachelib::get_volatile_pool("filenodes")
                .unwrap()
                .ok_or(Error::msg("no cache pool"))?,
            "filenodes",
            "",
        ))
    };

    let changesets = {
        let changesets: Arc<dyn Changesets> = Arc::new(DelayedChangesets::new(
            SqlChangesets::with_sqlite_in_memory()?,
            settings.db_get_dist,
            settings.db_put_dist,
        ));
        Arc::new(CachingChangesets::new(
            fb,
            changesets,
            cachelib::get_volatile_pool("changesets")
                .unwrap()
                .ok_or(Error::msg("no cache pool"))?,
        ))
    };

    let bonsai_globalrev_mapping = Arc::new(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?);

    let bonsai_hg_mapping = {
        let mapping: Arc<dyn BonsaiHgMapping> = Arc::new(DelayedBonsaiHgMapping::new(
            SqlBonsaiHgMapping::with_sqlite_in_memory()?,
            settings.db_get_dist,
            settings.db_put_dist,
        ));
        Arc::new(CachingBonsaiHgMapping::new(
            fb,
            mapping,
            cachelib::get_volatile_pool("bonsai_hg_mapping")
                .unwrap()
                .ok_or(Error::msg("no cache pool"))?,
        ))
    };

    // TODO:
    //  - add caching
    //  - add delay
    let bookmarks = Arc::new(SqlBookmarks::with_sqlite_in_memory()?);

    // Disable redaction check when executing benchmark reports
    let repoid = RepositoryId::new(rand::random());
    let blobstore =
        RepoBlobstoreArgs::new(blobstore, None, repoid, ScubaSampleBuilder::with_discard());
    Ok(BlobRepo::new(
        bookmarks,
        blobstore,
        filenodes,
        changesets,
        bonsai_globalrev_mapping,
        bonsai_hg_mapping,
        Arc::new(DummyLease {}),
        FilestoreConfig::default(),
    ))
}

/// Delay target future execution by delay sampled from provided distribution
fn delay<F, D>(distribution: D, target: F) -> impl Future<Item = F::Item, Error = Error>
where
    D: Distribution<f64>,
    F: Future<Error = Error>,
{
    future::lazy(move || {
        let seconds = rand::thread_rng().sample(distribution).abs();
        tokio_timer::sleep(Duration::new(
            seconds.trunc() as u64,
            (seconds.fract() * 1e+9) as u32,
        ))
        .from_err()
        .and_then(move |_| target)
    })
}

#[derive(Debug)]
struct DelayedBlobstore<B> {
    inner: B,
    get_dist: Normal,
    put_dist: Normal,
}

impl<B> DelayedBlobstore<B> {
    fn new(inner: B, get_dist: Normal, put_dist: Normal) -> Self {
        Self {
            inner,
            get_dist,
            put_dist,
        }
    }
}

impl<B: Blobstore> Blobstore for DelayedBlobstore<B> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        delay(self.get_dist, self.inner.get(ctx, key)).boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        delay(self.put_dist, self.inner.put(ctx, key, value)).boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        delay(self.get_dist, self.inner.is_present(ctx, key)).boxify()
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        delay(self.get_dist, self.inner.assert_present(ctx, key)).boxify()
    }
}

struct DelayedFilenodes<F> {
    inner: F,
    get_dist: Normal,
    put_dist: Normal,
}

impl<F> DelayedFilenodes<F> {
    fn new(inner: F, get_dist: Normal, put_dist: Normal) -> Self {
        Self {
            inner,
            get_dist,
            put_dist,
        }
    }
}

impl<F: Filenodes> Filenodes for DelayedFilenodes<F> {
    fn add_filenodes(
        &self,
        ctx: CoreContext,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error> {
        delay(self.put_dist, self.inner.add_filenodes(ctx, info, repo_id)).boxify()
    }

    fn add_or_replace_filenodes(
        &self,
        ctx: CoreContext,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error> {
        delay(
            self.put_dist,
            self.inner.add_or_replace_filenodes(ctx, info, repo_id),
        )
        .boxify()
    }

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        filenode: HgFileNodeId,
        repo_id: RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error> {
        delay(
            self.get_dist,
            self.inner.get_filenode(ctx, path, filenode, repo_id),
        )
        .boxify()
    }

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        repo_id: RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        delay(
            self.get_dist,
            self.inner.get_all_filenodes_maybe_stale(ctx, path, repo_id),
        )
        .boxify()
    }
}

struct DelayedChangesets<C> {
    inner: C,
    get_dist: Normal,
    put_dist: Normal,
}

impl<C> DelayedChangesets<C> {
    fn new(inner: C, get_dist: Normal, put_dist: Normal) -> Self {
        Self {
            inner,
            get_dist,
            put_dist,
        }
    }
}

impl<C: Changesets> Changesets for DelayedChangesets<C> {
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        delay(self.put_dist, self.inner.add(ctx, cs)).boxify()
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        delay(self.get_dist, self.inner.get(ctx, repo_id, cs_id)).boxify()
    }

    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
        delay(self.get_dist, self.inner.get_many(ctx, repo_id, cs_ids)).boxify()
    }
}

struct DelayedBonsaiHgMapping<M> {
    inner: M,
    get_dist: Normal,
    put_dist: Normal,
}

impl<M> DelayedBonsaiHgMapping<M> {
    fn new(inner: M, get_dist: Normal, put_dist: Normal) -> Self {
        Self {
            inner,
            get_dist,
            put_dist,
        }
    }
}

impl<M: BonsaiHgMapping> BonsaiHgMapping for DelayedBonsaiHgMapping<M> {
    fn add(&self, ctx: CoreContext, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        delay(self.put_dist, self.inner.add(ctx, entry)).boxify()
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetIds,
    ) -> BoxFuture<Vec<BonsaiHgMappingEntry>, Error> {
        delay(self.get_dist, self.inner.get(ctx, repo_id, cs_id)).boxify()
    }
}
