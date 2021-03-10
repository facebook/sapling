/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bookmarks::{BookmarkName, Bookmarks};
use bulkops::PublicChangesetBulkFetch;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use dag::InProcessIdDag;
use fbinit::FacebookInit;
use mononoke_types::RepositoryId;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};
use sql_ext::SqlConnections;

use crate::iddag::IdDagSaveStore;
use crate::idmap::{
    CacheHandlers, CachedIdMap, ConcurrentMemIdMap, IdMap, IdMapFactory, SqlIdMap,
    SqlIdMapVersionStore,
};
use crate::manager::{PeriodicReloadSegmentedChangelog, SegmentedChangelogManager};
use crate::on_demand::{OnDemandUpdateSegmentedChangelog, PeriodicUpdateSegmentedChangelog};
use crate::owned::OwnedSegmentedChangelog;
use crate::seeder::SegmentedChangelogSeeder;
use crate::tailer::SegmentedChangelogTailer;
use crate::types::IdMapVersion;
use crate::version_store::SegmentedChangelogVersionStore;
use crate::DisabledSegmentedChangelog;

/// SegmentedChangelog instatiation helper.
/// It works together with SegmentedChangelogConfig and BlobRepoFactory to produce a
/// SegmentedChangelog.
/// Config options:
/// Enabled = false -> DisabledSegmentedChangelog
/// Enabled = true
///   update_algorithm = 'ondemand' -> OnDemandUpdateSegmentedChangelog
#[derive(Default, Clone)]
pub struct SegmentedChangelogBuilder {
    connections: Option<SqlConnections>,
    repo_id: Option<RepositoryId>,
    idmap_version: Option<IdMapVersion>,
    replica_lag_monitor: Option<Arc<dyn ReplicaLagMonitor>>,
    changeset_fetcher: Option<Arc<dyn ChangesetFetcher>>,
    changeset_bulk_fetch: Option<Arc<PublicChangesetBulkFetch>>,
    blobstore: Option<Arc<dyn Blobstore>>,
    bookmarks: Option<Arc<dyn Bookmarks>>,
    bookmark_name: Option<BookmarkName>,
    cache_handlers: Option<CacheHandlers>,
    update_to_bookmark_period: Option<Duration>,
    reload_dag_period: Option<Duration>,
}

impl SqlConstruct for SegmentedChangelogBuilder {
    const LABEL: &'static str = "segmented_changelog";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-segmented-changelog.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connections: Some(connections),
            repo_id: None,
            idmap_version: None,
            replica_lag_monitor: None,
            changeset_fetcher: None,
            changeset_bulk_fetch: None,
            blobstore: None,
            bookmarks: None,
            bookmark_name: None,
            cache_handlers: None,
            update_to_bookmark_period: None,
            reload_dag_period: None,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SegmentedChangelogBuilder {}

impl SegmentedChangelogBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_manager(mut self) -> Result<SegmentedChangelogManager> {
        Ok(SegmentedChangelogManager::new(
            self.repo_id()?,
            self.build_segmented_changelog_version_store()?,
            self.build_iddag_save_store()?,
            self.build_idmap_factory()?,
            self.changeset_fetcher()?,
            self.bookmarks()?,
            self.bookmark_name()?,
            self.update_to_bookmark_period.take(),
        ))
    }

    pub fn build_disabled(self) -> DisabledSegmentedChangelog {
        DisabledSegmentedChangelog::new()
    }

    pub fn build_on_demand_update(mut self) -> Result<OnDemandUpdateSegmentedChangelog> {
        let owned = self.new_owned()?;
        let changeset_fetcher = self.changeset_fetcher()?;
        Ok(OnDemandUpdateSegmentedChangelog::new(
            owned.iddag,
            owned.idmap,
            changeset_fetcher,
        ))
    }

    pub async fn build_on_demand_update_start_from_save(
        mut self,
        ctx: &CoreContext,
    ) -> Result<OnDemandUpdateSegmentedChangelog> {
        let changeset_fetcher = self.changeset_fetcher()?;
        let manager = self.build_manager()?;
        let owned = manager.load_owned(ctx).await?;
        Ok(OnDemandUpdateSegmentedChangelog::new(
            owned.iddag,
            owned.idmap,
            changeset_fetcher,
        ))
    }

    pub fn build_periodic_update(
        mut self,
        ctx: &CoreContext,
    ) -> Result<PeriodicUpdateSegmentedChangelog> {
        let owned = self.new_owned()?;
        let changeset_fetcher = self.changeset_fetcher()?;
        let dag = PeriodicUpdateSegmentedChangelog::for_bookmark(
            ctx,
            Arc::new(OnDemandUpdateSegmentedChangelog::from_owned(
                owned,
                changeset_fetcher,
            )),
            self.bookmarks()?,
            self.bookmark_name()?,
            self.update_to_bookmark_period()?,
        );
        Ok(dag)
    }

    pub async fn build_periodic_reload(
        mut self,
        ctx: &CoreContext,
    ) -> Result<PeriodicReloadSegmentedChangelog> {
        let reload_dag_period = self.reload_dag_period()?;
        let manager = self.build_manager()?;
        PeriodicReloadSegmentedChangelog::start(ctx, manager, reload_dag_period).await
    }

    pub async fn build_seeder(mut self, ctx: &CoreContext) -> Result<SegmentedChangelogSeeder> {
        let idmap_version_store = self.build_sql_idmap_version_store()?;
        if self.idmap_version.is_none() {
            let version = match idmap_version_store
                .get(&ctx)
                .await
                .context("getting idmap version from store")?
            {
                Some(v) => v.0 + 1,
                None => 1,
            };
            self.idmap_version = Some(IdMapVersion(version));
        }
        let seeder = SegmentedChangelogSeeder::new(
            self.idmap_version(),
            idmap_version_store,
            self.changeset_bulk_fetch()?,
            self.build_segmented_changelog_version_store()?,
            self.build_iddag_save_store()?,
            self.build_idmap_factory()?,
        );
        Ok(seeder)
    }

    pub fn build_tailer(mut self) -> Result<SegmentedChangelogTailer> {
        let tailer = SegmentedChangelogTailer::new(
            self.repo_id()?,
            self.changeset_fetcher()?,
            self.bookmarks()?,
            self.bookmark_name()?,
            self.build_segmented_changelog_version_store()?,
            self.build_iddag_save_store()?,
            self.build_idmap_factory()?,
        );
        Ok(tailer)
    }

    pub fn with_sql_connections(mut self, connections: SqlConnections) -> Self {
        self.connections = Some(connections);
        self
    }

    pub fn with_repo_id(mut self, repo_id: RepositoryId) -> Self {
        self.repo_id = Some(repo_id);
        self
    }

    pub fn with_idmap_version(mut self, version: u64) -> Self {
        self.idmap_version = Some(IdMapVersion(version));
        self
    }

    pub fn with_replica_lag_monitor(
        mut self,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    ) -> Self {
        self.replica_lag_monitor = Some(replica_lag_monitor);
        self
    }

    pub fn with_changeset_fetcher(mut self, changeset_fetcher: Arc<dyn ChangesetFetcher>) -> Self {
        self.changeset_fetcher = Some(changeset_fetcher);
        self
    }

    pub fn with_changeset_bulk_fetch(
        mut self,
        changeset_bulk_fetch: Arc<PublicChangesetBulkFetch>,
    ) -> Self {
        self.changeset_bulk_fetch = Some(changeset_bulk_fetch);
        self
    }

    pub fn with_blobstore(mut self, blobstore: Arc<dyn Blobstore>) -> Self {
        self.blobstore = Some(blobstore);
        self
    }

    pub fn with_bookmarks(mut self, bookmarks: Arc<dyn Bookmarks>) -> Self {
        self.bookmarks = Some(bookmarks);
        self
    }

    pub fn with_bookmark_name(mut self, bookmark_name: BookmarkName) -> Self {
        self.bookmark_name = Some(bookmark_name);
        self
    }

    pub fn with_caching(
        mut self,
        fb: FacebookInit,
        cache_pool: cachelib::VolatileLruCachePool,
    ) -> Self {
        self.cache_handlers = Some(CacheHandlers::prod(fb, cache_pool));
        self
    }

    pub fn with_cache_handlers(mut self, cache_handlers: CacheHandlers) -> Self {
        self.cache_handlers = Some(cache_handlers);
        self
    }

    pub fn with_update_to_bookmark_period(mut self, period: Duration) -> Self {
        self.update_to_bookmark_period = Some(period);
        self
    }

    pub fn with_reload_dag_period(mut self, period: Duration) -> Self {
        self.reload_dag_period = Some(period);
        self
    }

    pub fn with_blobrepo(self, repo: &BlobRepo) -> Self {
        let repo_id = repo.get_repoid();
        let changesets = repo.get_changesets_object();
        let phases = repo.get_phases();
        let bulk_fetch = PublicChangesetBulkFetch::new(repo_id, changesets, phases);
        self.with_repo_id(repo_id)
            .with_changeset_fetcher(repo.get_changeset_fetcher())
            .with_bookmarks(repo.bookmarks())
            .with_blobstore(Arc::new(repo.get_blobstore()))
            .with_changeset_bulk_fetch(Arc::new(bulk_fetch))
    }

    pub(crate) fn new_owned(&mut self) -> Result<OwnedSegmentedChangelog> {
        let iddag = InProcessIdDag::new_in_process();
        let idmap: Arc<dyn IdMap> = Arc::new(ConcurrentMemIdMap::new());
        Ok(OwnedSegmentedChangelog::new(iddag, idmap))
    }

    #[allow(dead_code)]
    pub(crate) fn build_idmap(&mut self) -> Result<Arc<dyn IdMap>> {
        let mut idmap: Arc<dyn IdMap> = Arc::new(self.build_sql_idmap()?);
        if let Some(cache_handlers) = self.cache_handlers.take() {
            idmap = Arc::new(CachedIdMap::new(
                idmap,
                cache_handlers,
                self.repo_id()?,
                self.idmap_version(),
            ));
        }
        Ok(idmap)
    }

    #[allow(dead_code)]
    pub(crate) fn build_sql_idmap(&mut self) -> Result<SqlIdMap> {
        let connections = self.connections_clone()?;
        let replica_lag_monitor = self.replica_lag_monitor();
        let repo_id = self.repo_id()?;
        let idmap_version = self.idmap_version();
        Ok(SqlIdMap::new(
            connections,
            replica_lag_monitor,
            repo_id,
            idmap_version,
        ))
    }

    pub(crate) fn build_idmap_factory(&mut self) -> Result<IdMapFactory> {
        let connections = self.connections_clone()?;
        let replica_lag_monitor = self.replica_lag_monitor();
        let repo_id = self.repo_id()?;
        let mut idmap_factory = IdMapFactory::new(connections, replica_lag_monitor, repo_id);
        if let Some(cache_handlers) = self.cache_handlers.take() {
            idmap_factory = idmap_factory.with_cache_handlers(cache_handlers);
        }
        Ok(idmap_factory)
    }

    pub(crate) fn build_sql_idmap_version_store(&self) -> Result<SqlIdMapVersionStore> {
        let connections = self.connections_clone()?;
        let repo_id = self.repo_id()?;
        Ok(SqlIdMapVersionStore::new(connections, repo_id))
    }

    pub(crate) fn build_segmented_changelog_version_store(
        &self,
    ) -> Result<SegmentedChangelogVersionStore> {
        let connections = self.connections_clone()?;
        let repo_id = self.repo_id()?;
        Ok(SegmentedChangelogVersionStore::new(connections, repo_id))
    }

    pub(crate) fn build_iddag_save_store(&mut self) -> Result<IdDagSaveStore> {
        let blobstore = self.blobstore()?;
        let repo_id = self.repo_id()?;
        Ok(IdDagSaveStore::new(repo_id, blobstore))
    }

    fn repo_id(&self) -> Result<RepositoryId> {
        self.repo_id.ok_or_else(|| {
            format_err!("SegmentedChangelog cannot be built without RepositoryId being specified.")
        })
    }

    fn idmap_version(&self) -> IdMapVersion {
        self.idmap_version.unwrap_or_default()
    }

    fn replica_lag_monitor(&mut self) -> Arc<dyn ReplicaLagMonitor> {
        self.replica_lag_monitor
            .take()
            .unwrap_or_else(|| Arc::new(NoReplicaLagMonitor()))
    }

    fn changeset_fetcher(&mut self) -> Result<Arc<dyn ChangesetFetcher>> {
        self.changeset_fetcher.take().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without ChangesetFetcher being specified."
            )
        })
    }

    fn changeset_bulk_fetch(&mut self) -> Result<Arc<PublicChangesetBulkFetch>> {
        self.changeset_bulk_fetch.take().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without ChangesetBulkFetch being specified."
            )
        })
    }

    fn connections_clone(&self) -> Result<SqlConnections> {
        let connections = self.connections.as_ref().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without SqlConnections being specified."
            )
        })?;
        Ok(connections.clone())
    }

    fn blobstore(&mut self) -> Result<Arc<dyn Blobstore>> {
        self.blobstore.take().ok_or_else(|| {
            format_err!("SegmentedChangelog cannot be built without Blobstore being specified.")
        })
    }

    fn bookmarks(&mut self) -> Result<Arc<dyn Bookmarks>> {
        self.bookmarks.take().ok_or_else(|| {
            format_err!("SegmentedChangelog cannot be built without Bookmarks being specified.")
        })
    }

    fn bookmark_name(&mut self) -> Result<BookmarkName> {
        if let Some(name) = &self.bookmark_name {
            Ok(name.clone())
        } else {
            BookmarkName::new("master")
        }
    }

    fn update_to_bookmark_period(&mut self) -> Result<Duration> {
        self.update_to_bookmark_period.take().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without \
                update_to_bookmark_period being specified."
            )
        })
    }

    fn reload_dag_period(&mut self) -> Result<Duration> {
        self.reload_dag_period.take().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without \
                reload_dag_period being specified."
            )
        })
    }
}
