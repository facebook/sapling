/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Main function is `new_benchmark_repo` which creates `BlobRepo` which delay applied
//! to all underlying stores, but which all the caching enabled.

use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bonsai_git_mapping::{ArcBonsaiGitMapping, SqlBonsaiGitMappingConnection};
use bonsai_globalrev_mapping::{ArcBonsaiGlobalrevMapping, SqlBonsaiGlobalrevMapping};
use bonsai_hg_mapping::{
    ArcBonsaiHgMapping, BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds,
    CachingBonsaiHgMapping, SqlBonsaiHgMappingBuilder,
};
use bonsai_svnrev_mapping::{
    ArcRepoBonsaiSvnrevMapping, RepoBonsaiSvnrevMapping, SqlBonsaiSvnrevMapping,
};
use bookmarks::{ArcBookmarkUpdateLog, ArcBookmarks};
use cacheblob::{dummy::DummyLease, new_cachelib_blobstore, CachelibBlobstoreOptions};
use changeset_fetcher::{ArcChangesetFetcher, SimpleChangesetFetcher};
use changesets::{
    ArcChangesets, CachingChangesets, ChangesetEntry, ChangesetInsert, Changesets, SqlChangesets,
    SqlChangesetsBuilder,
};
use context::CoreContext;
use dbbookmarks::{ArcSqlBookmarks, SqlBookmarksBuilder};
use delayblob::DelayedBlobstore;
use fbinit::FacebookInit;
use filenodes::{
    ArcFilenodes, FilenodeInfo, FilenodeRangeResult, FilenodeResult, Filenodes, PreparedFilenode,
};
use filestore::{ArcFilestoreConfig, FilestoreConfig};
use futures::future::{FutureExt as _, TryFutureExt as _};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::Future;
use memblob::Memblob;
use mercurial_mutation::{ArcHgMutationStore, SqlHgMutationStoreBuilder};
use mercurial_types::{HgChangesetIdPrefix, HgChangesetIdsResolvedFromPrefix, HgFileNodeId};
use metaconfig_types::ArcRepoConfig;
use mononoke_types::{
    ChangesetId, ChangesetIdPrefix, ChangesetIdsResolvedFromPrefix, RepoPath, RepositoryId,
};
use newfilenodes::NewFilenodesBuilder;
use phases::{ArcSqlPhasesFactory, SqlPhasesFactory};
use rand::Rng;
use rand_distr::Distribution;
use rendezvous::RendezVousOptions;
use repo_blobstore::{ArcRepoBlobstore, RepoBlobstoreArgs};
use repo_derived_data::{ArcRepoDerivedData, RepoDerivedData};
use repo_identity::{ArcRepoIdentity, RepoIdentity};
use scuba_ext::MononokeScubaSampleBuilder;
use segmented_changelog::DisabledSegmentedChangelog;
use segmented_changelog_types::ArcSegmentedChangelog;
use sql_construct::SqlConstruct;
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

pub struct BenchmarkRepoFactory {
    fb: FacebookInit,
    delay_settings: DelaySettings,
}

impl BenchmarkRepoFactory {
    pub fn new(fb: FacebookInit, delay_settings: DelaySettings) -> Self {
        BenchmarkRepoFactory { fb, delay_settings }
    }
}

fn cache_pool(name: &str) -> Result<cachelib::LruCachePool> {
    Ok(cachelib::get_pool(name).ok_or_else(|| anyhow!("no cache pool: {}", name))?)
}

fn volatile_pool(name: &str) -> Result<cachelib::VolatileLruCachePool> {
    Ok(cachelib::get_volatile_pool(name)?.ok_or_else(|| anyhow!("no cache pool: {}", name))?)
}

#[facet::factory()]
impl BenchmarkRepoFactory {
    pub fn repo_blobstore(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcRepoBlobstore> {
        let blobstore: Arc<dyn Blobstore> = Arc::new(DelayedBlobstore::new(
            Memblob::default(),
            self.delay_settings.blobstore_get_dist,
            self.delay_settings.blobstore_put_dist,
        ));
        let blobstore = Arc::new(new_cachelib_blobstore(
            blobstore,
            Arc::new(cache_pool("blobstore-blobs")?),
            Arc::new(cache_pool("blobstore-presence")?),
            CachelibBlobstoreOptions::default(),
        ));
        let args = RepoBlobstoreArgs::new(
            blobstore,
            None,
            repo_identity.id(),
            MononokeScubaSampleBuilder::with_discard(),
        );
        let (repo_blobstore, _) = args.into_blobrepo_parts();
        Ok(Arc::new(repo_blobstore))
    }

    pub fn repo_config(&self, repo_identity: &ArcRepoIdentity) -> ArcRepoConfig {
        let mut config = test_repo_factory::default_test_repo_config();
        config.repoid = repo_identity.id();
        Arc::new(config)
    }

    pub fn repo_identity(&self) -> ArcRepoIdentity {
        Arc::new(RepoIdentity::new(
            RepositoryId::new(rand::random()),
            "benchmarkrepo".to_string(),
        ))
    }

    pub fn changesets(&self) -> Result<ArcChangesets> {
        let changesets: Arc<dyn Changesets> = Arc::new(DelayedChangesets::new(
            SqlChangesetsBuilder::with_sqlite_in_memory()?.build(RendezVousOptions::for_test()),
            self.delay_settings.db_get_dist,
            self.delay_settings.db_put_dist,
        ));
        Ok(Arc::new(CachingChangesets::new(
            self.fb,
            changesets,
            volatile_pool("changesets")?,
        )))
    }

    pub fn changeset_fetcher(
        &self,
        repo_identity: &ArcRepoIdentity,
        changesets: &ArcChangesets,
    ) -> ArcChangesetFetcher {
        Arc::new(SimpleChangesetFetcher::new(
            changesets.clone(),
            repo_identity.id(),
        ))
    }

    pub fn sql_bookmarks(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcSqlBookmarks> {
        // TODO:
        //  - add caching
        //  - add delay
        Ok(Arc::new(
            SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(repo_identity.id()),
        ))
    }

    pub fn bookmarks(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarks {
        sql_bookmarks.clone()
    }

    pub fn bookmark_update_log(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarkUpdateLog {
        sql_bookmarks.clone()
    }

    pub fn sql_phases_factory(&self) -> Result<ArcSqlPhasesFactory> {
        Ok(Arc::new(SqlPhasesFactory::with_sqlite_in_memory()?))
    }

    pub fn bonsai_hg_mapping(&self) -> Result<ArcBonsaiHgMapping> {
        let mapping: Arc<dyn BonsaiHgMapping> = Arc::new(DelayedBonsaiHgMapping::new(
            SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()?
                .build(RendezVousOptions::for_test()),
            self.delay_settings.db_get_dist,
            self.delay_settings.db_put_dist,
        ));
        Ok(Arc::new(CachingBonsaiHgMapping::new(
            self.fb,
            mapping,
            volatile_pool("bonsai_hg_mapping")?,
        )))
    }

    pub fn bonsai_git_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiGitMapping> {
        Ok(Arc::new(
            SqlBonsaiGitMappingConnection::with_sqlite_in_memory()?
                .with_repo_id(repo_identity.id()),
        ))
    }

    pub fn bonsai_globalrev_mapping(&self) -> Result<ArcBonsaiGlobalrevMapping> {
        Ok(Arc::new(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?))
    }

    pub fn repo_bonsai_svnrev_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcRepoBonsaiSvnrevMapping> {
        Ok(Arc::new(RepoBonsaiSvnrevMapping::new(
            repo_identity.id(),
            Arc::new(SqlBonsaiSvnrevMapping::with_sqlite_in_memory()?),
        )))
    }

    pub fn filenodes(&self) -> Result<ArcFilenodes> {
        let pool = volatile_pool("filenodes")?;

        let mut builder = NewFilenodesBuilder::with_sqlite_in_memory()?;
        builder.enable_caching(self.fb, pool.clone(), pool, "filenodes", "");

        Ok(Arc::new(DelayedFilenodes::new(
            builder.build(),
            self.delay_settings.db_get_dist,
            self.delay_settings.db_put_dist,
        )))
    }

    pub fn hg_mutation_store(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcHgMutationStore> {
        Ok(Arc::new(
            SqlHgMutationStoreBuilder::with_sqlite_in_memory()?.with_repo_id(repo_identity.id()),
        ))
    }

    pub fn segmented_changelog(&self) -> ArcSegmentedChangelog {
        Arc::new(DisabledSegmentedChangelog::new())
    }

    pub fn repo_derived_data(&self, repo_config: &ArcRepoConfig) -> ArcRepoDerivedData {
        Arc::new(RepoDerivedData::new(
            repo_config.derived_data_config.clone(),
            Arc::new(DummyLease {}),
        ))
    }

    pub fn filestore_config(&self) -> ArcFilestoreConfig {
        Arc::new(FilestoreConfig::default())
    }
}

pub fn new_benchmark_repo(fb: FacebookInit, settings: DelaySettings) -> Result<BlobRepo> {
    let repo = BenchmarkRepoFactory::new(fb, settings).build()?;
    Ok(repo)
}

/// Delay target future execution by delay sampled from provided distribution
fn delay<F, D>(distribution: D, target: F) -> impl Future<Item = F::Item, Error = Error>
where
    D: Distribution<f64>,
    F: Future<Error = Error>,
{
    let seconds = rand::thread_rng().sample(distribution).abs();

    tokio_shim::time::sleep(Duration::new(
        seconds.trunc() as u64,
        (seconds.fract() * 1e+9) as u32,
    ))
    .map(Result::<_, Error>::Ok)
    .boxed()
    .compat()
    .and_then(move |_| target)
}

async fn delay_v2(distribution: impl Distribution<f64>) {
    let seconds = rand::thread_rng().sample(distribution).abs();
    let duration = Duration::new(seconds.trunc() as u64, (seconds.fract() * 1e+9) as u32);
    tokio::time::delay_for(duration).await;
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
        info: Vec<PreparedFilenode>,
        repo_id: RepositoryId,
    ) -> BoxFuture<FilenodeResult<()>, Error> {
        delay(self.put_dist, self.inner.add_filenodes(ctx, info, repo_id)).boxify()
    }

    fn add_or_replace_filenodes(
        &self,
        ctx: CoreContext,
        info: Vec<PreparedFilenode>,
        repo_id: RepositoryId,
    ) -> BoxFuture<FilenodeResult<()>, Error> {
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
    ) -> BoxFuture<FilenodeResult<Option<FilenodeInfo>>, Error> {
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
        limit: Option<u64>,
    ) -> BoxFuture<FilenodeRangeResult<Vec<FilenodeInfo>>, Error> {
        delay(
            self.get_dist,
            self.inner
                .get_all_filenodes_maybe_stale(ctx, path, repo_id, limit),
        )
        .boxify()
    }

    fn prime_cache(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        filenodes: &[PreparedFilenode],
    ) {
        self.inner.prime_cache(ctx, repo_id, filenodes)
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

#[async_trait]
impl<C: Changesets> Changesets for DelayedChangesets<C> {
    async fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> Result<bool, Error> {
        delay_v2(self.put_dist).await;
        self.inner.add(ctx, cs).await
    }

    async fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        delay_v2(self.get_dist).await;
        self.inner.get(ctx, repo_id, cs_id).await
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        delay_v2(self.get_dist).await;
        self.inner.get_many(ctx, repo_id, cs_ids).await
    }

    async fn get_many_by_prefix(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
        delay_v2(self.get_dist).await;
        self.inner
            .get_many_by_prefix(ctx, repo_id, cs_prefix, limit)
            .await
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.inner.prime_cache(ctx, changesets)
    }

    fn get_sql_changesets(&self) -> &SqlChangesets {
        self.inner.get_sql_changesets()
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

#[async_trait]
impl<M: BonsaiHgMapping> BonsaiHgMapping for DelayedBonsaiHgMapping<M> {
    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        delay_v2(self.put_dist).await;
        self.inner.add(ctx, entry).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error> {
        delay_v2(self.get_dist).await;
        self.inner.get(ctx, repo_id, cs_id).await
    }

    async fn get_many_hg_by_prefix(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_prefix: HgChangesetIdPrefix,
        limit: usize,
    ) -> Result<HgChangesetIdsResolvedFromPrefix, Error> {
        delay_v2(self.get_dist).await;
        self.inner
            .get_many_hg_by_prefix(ctx, repo_id, cs_prefix, limit)
            .await
    }
}
