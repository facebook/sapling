/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Error, Result};
use fbinit::FacebookInit;
use futures::stream::{self, Stream, TryStreamExt};
use futures_stats::TimedFutureExt;
use slog::{debug, error, info};
use sql_ext::facebook::{MyAdmin, MysqlOptions};
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

use stats::prelude::*;

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use bookmarks::Bookmarks;
use bulkops::{Direction, PublicChangesetBulkFetch};
use changeset_fetcher::{ChangesetFetcher, PrefetchedChangesetsFetcher};
use changesets::{ChangesetEntry, ChangesetsArc};
use context::CoreContext;
use metaconfig_types::MetadataDatabaseConfig;
use mononoke_types::{Generation, RepositoryId};
use phases::PhasesArc;

use crate::dag::ops::DagAddHeads;
use crate::dag::DagAlgorithm;
use crate::iddag::IdDagSaveStore;
use crate::idmap::{cs_id_from_vertex_name, CacheHandlers, IdMapFactory, SqlIdMapVersionStore};
use crate::owned::OwnedSegmentedChangelog;
use crate::parents::FetchParents;
use crate::types::{IdMapVersion, SegmentedChangelogVersion};
use crate::update::{server_namedag, vertexlist_from_seedheads, SeedHead};
use crate::version_store::SegmentedChangelogVersionStore;
use crate::InProcessIdDag;
use crate::SegmentedChangelogSqlConnections;

define_stats! {
    prefix = "mononoke.segmented_changelog.tailer.update";
    count: timeseries(Sum),
    failure: timeseries(Sum),
    success: timeseries(Sum),
    duration_ms:
        histogram(1000, 0, 60_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    count_per_repo: dynamic_timeseries("{}.count", (repo_id: i32); Sum),
    failure_per_repo: dynamic_timeseries("{}.failure", (repo_id: i32); Sum),
    success_per_repo: dynamic_timeseries("{}.success", (repo_id: i32); Sum),
    duration_ms_per_repo: dynamic_histogram(
        "{}.duration_ms", (repo_id: i32);
        1000, 0, 60_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99
    ),
}

pub struct SegmentedChangelogTailer {
    repo_id: RepositoryId,
    changeset_fetcher: Arc<PrefetchedChangesetsFetcher>,
    bulk_fetch: Arc<PublicChangesetBulkFetch>,
    bookmarks: Arc<dyn Bookmarks>,
    seed_heads: Vec<SeedHead>,
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
    idmap_version_store: SqlIdMapVersionStore,
}

impl SegmentedChangelogTailer {
    pub fn new(
        repo_id: RepositoryId,
        connections: SegmentedChangelogSqlConnections,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        changeset_fetcher: Arc<PrefetchedChangesetsFetcher>,
        bulk_fetch: Arc<PublicChangesetBulkFetch>,
        blobstore: Arc<dyn Blobstore>,
        bookmarks: Arc<dyn Bookmarks>,
        seed_heads: Vec<SeedHead>,
        caching: Option<(FacebookInit, cachelib::VolatileLruCachePool)>,
    ) -> Self {
        let sc_version_store = SegmentedChangelogVersionStore::new(connections.0.clone(), repo_id);
        let idmap_version_store = SqlIdMapVersionStore::new(connections.0.clone(), repo_id);
        let iddag_save_store = IdDagSaveStore::new(repo_id, blobstore);
        let mut idmap_factory = IdMapFactory::new(connections.0, replica_lag_monitor, repo_id);
        if let Some((fb, pool)) = caching {
            let cache_handlers = CacheHandlers::prod(fb, pool);
            idmap_factory = idmap_factory.with_cache_handlers(cache_handlers);
        }
        Self {
            repo_id,
            changeset_fetcher,
            bulk_fetch,
            bookmarks,
            seed_heads,
            sc_version_store,
            iddag_save_store,
            idmap_factory,
            idmap_version_store,
        }
    }

    pub async fn build_from(
        ctx: &CoreContext,
        blobrepo: &BlobRepo,
        storage_config_metadata: &MetadataDatabaseConfig,
        mysql_options: &MysqlOptions,
        seed_heads: Vec<SeedHead>,
        prefetched_commits: impl Stream<Item = Result<ChangesetEntry, Error>>,
        caching: Option<(FacebookInit, cachelib::VolatileLruCachePool)>,
    ) -> Result<Self> {
        let repo_id = blobrepo.get_repoid();

        let db_address = match storage_config_metadata {
            MetadataDatabaseConfig::Local(_) => None,
            MetadataDatabaseConfig::Remote(remote_config) => {
                Some(remote_config.primary.db_address.clone())
            }
        };
        let replica_lag_monitor: Arc<dyn ReplicaLagMonitor> = match db_address {
            None => Arc::new(NoReplicaLagMonitor()),
            Some(address) => {
                let my_admin = MyAdmin::new(ctx.fb).context("building myadmin client")?;
                Arc::new(my_admin.single_shard_lag_monitor(address))
            }
        };

        let sql_factory = make_metadata_sql_factory(
            ctx.fb,
            storage_config_metadata.clone(),
            mysql_options.clone(),
            ReadOnlyStorage(false),
        )
        .await
        .with_context(|| format!("repo {}: constructing metadata sql factory", repo_id))?;

        let segmented_changelog_sql_connections = sql_factory
            .open::<SegmentedChangelogSqlConnections>()
            .with_context(|| {
                format!(
                    "repo {}: error constructing segmented changelog sql connections",
                    repo_id
                )
            })?;

        let changeset_fetcher = Arc::new(
            PrefetchedChangesetsFetcher::new(
                repo_id,
                blobrepo.changesets_arc(),
                prefetched_commits,
            )
            .await?,
        );

        let bulk_fetcher = Arc::new(PublicChangesetBulkFetch::new(
            blobrepo.changesets_arc(),
            blobrepo.phases_arc(),
        ));

        Ok(SegmentedChangelogTailer::new(
            repo_id,
            segmented_changelog_sql_connections,
            replica_lag_monitor,
            changeset_fetcher,
            bulk_fetcher,
            Arc::new(blobrepo.get_blobstore()),
            Arc::clone(blobrepo.bookmarks()) as Arc<dyn Bookmarks>,
            seed_heads,
            caching,
        ))
    }

    pub async fn run(&self, ctx: &CoreContext, period: Duration) {
        STATS::success.add_value(0);
        STATS::success_per_repo.add_value(0, (self.repo_id.id(),));

        let mut interval = tokio::time::interval(period);
        loop {
            let _ = interval.tick().await;
            debug!(ctx.logger(), "repo {}: woke up to update", self.repo_id,);

            STATS::count.add_value(1);
            STATS::count_per_repo.add_value(1, (self.repo_id.id(),));

            let (stats, update_result) = self.once(&ctx, false).timed().await;

            STATS::duration_ms.add_value(stats.completion_time.as_millis() as i64);
            STATS::duration_ms_per_repo.add_value(
                stats.completion_time.as_millis() as i64,
                (self.repo_id.id(),),
            );

            let mut scuba = ctx.scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.add("repo_id", self.repo_id.id());
            scuba.add("success", update_result.is_ok());

            let msg = match update_result {
                Ok(_) => {
                    STATS::success.add_value(1);
                    STATS::success_per_repo.add_value(1, (self.repo_id.id(),));
                    None
                }
                Err(err) => {
                    STATS::failure.add_value(1);
                    STATS::failure_per_repo.add_value(1, (self.repo_id.id(),));
                    error!(
                        ctx.logger(),
                        "repo {}: failed to incrementally update segmented changelog: {:?}",
                        self.repo_id,
                        err
                    );
                    Some(format!("{:?}", err))
                }
            };
            scuba.log_with_msg("segmented_changelog_tailer_update", msg);
        }
    }

    pub async fn once(
        &self,
        ctx: &CoreContext,
        force_reseed: bool,
    ) -> Result<OwnedSegmentedChangelog> {
        info!(
            ctx.logger(),
            "repo {}: starting incremental update to segmented changelog", self.repo_id,
        );

        let (seeding, idmap_version, iddag) = {
            let sc_version = self.sc_version_store.get(&ctx).await.with_context(|| {
                format!(
                    "repo {}: error loading segmented changelog version",
                    self.repo_id
                )
            })?;

            match sc_version {
                Some(sc_version) => {
                    if force_reseed {
                        (
                            true,
                            sc_version.idmap_version.bump(),
                            InProcessIdDag::new_in_process(),
                        )
                    } else {
                        let iddag = self
                            .iddag_save_store
                            .load(&ctx, sc_version.iddag_version)
                            .await
                            .with_context(|| {
                                format!("repo {}: failed to load iddag", self.repo_id)
                            })?;
                        (false, sc_version.idmap_version, iddag)
                    }
                }
                None => (true, IdMapVersion(1), InProcessIdDag::new_in_process()),
            }
        };
        let idmap = self.idmap_factory.for_writer(ctx, idmap_version);

        let mut namedag = server_namedag(ctx.clone(), iddag, idmap)?;

        let heads =
            vertexlist_from_seedheads(&ctx, &self.seed_heads, self.bookmarks.as_ref()).await?;

        let head_commits: Vec<_> = namedag
            .heads(namedag.master_group().await?)
            .await?
            .iter()
            .await?
            .map_ok(|name| cs_id_from_vertex_name(&name))
            .try_collect()
            .await?;

        let changeset_fetcher = {
            let namedag_max_gen = stream::iter(head_commits.iter().map(Ok::<_, Error>))
                .try_fold(0, {
                    let fetcher = &self.changeset_fetcher;
                    move |max, cs_id| async move {
                        let gen = fetcher.get_generation_number(ctx.clone(), *cs_id).await?;
                        Ok(max.max(gen.value()))
                    }
                })
                .await?;
            let heads_min_gen = stream::iter(
                heads
                    .vertexes()
                    .iter()
                    .map(|name| Ok::<_, Error>(cs_id_from_vertex_name(name))),
            )
            .try_fold(Generation::max_gen().value(), {
                let fetcher = &self.changeset_fetcher;
                move |min, cs_id| async move {
                    let gen = fetcher.get_generation_number(ctx.clone(), cs_id).await?;
                    Ok(min.min(gen.value()))
                }
            })
            .await?;

            if heads_min_gen.saturating_sub(namedag_max_gen) > 1000 {
                // This has the potential to cause OOM by fetching a large
                // chunk of the repo
                let missing = self.bulk_fetch.fetch_bounded(
                    &ctx,
                    Direction::NewestFirst,
                    Some(
                        self.bulk_fetch
                            .get_repo_bounds_after_commits(&ctx, head_commits)
                            .await?,
                    ),
                );
                Arc::new(self.changeset_fetcher.clone_with_extension(missing).await?)
            } else {
                self.changeset_fetcher.clone()
            }
        };

        let parent_fetcher = FetchParents::new(ctx.clone(), changeset_fetcher);

        // Note on memory use: we do not flush the changes out in the middle
        // of writing to the IdMap.
        // Thus, if OOMs happen here, the IdMap may need to flush writes to the DB
        // at interesting points.
        let changed = namedag.add_heads(&parent_fetcher, &heads).await?;

        let (idmap, iddag) = namedag.into_idmap_dag();
        let idmap = idmap.finish().await?;

        if !changed {
            info!(
                ctx.logger(),
                "repo {}: segmented changelog already up to date, skipping update to iddag",
                self.repo_id
            );
            let owned = OwnedSegmentedChangelog::new(iddag, idmap);
            return Ok(owned);
        }

        info!(
            ctx.logger(),
            "repo {}: IdMap updated, IdDag updated", self.repo_id
        );

        // Save the IdDag
        let iddag_version = self
            .iddag_save_store
            .save(&ctx, &iddag)
            .await
            .with_context(|| format!("repo {}: error saving iddag", self.repo_id))?;

        // Update SegmentedChangelogVersion
        let sc_version = SegmentedChangelogVersion::new(iddag_version, idmap_version);
        if seeding {
            self.sc_version_store
                .set(&ctx, sc_version)
                .await
                .with_context(|| {
                    format!(
                        "repo {}: error updating segmented changelog version store",
                        self.repo_id
                    )
                })?;
            info!(
                ctx.logger(),
                "repo {}: successfully seeded segmented changelog", self.repo_id,
            );
        } else {
            self.sc_version_store
                .update(&ctx, sc_version)
                .await
                .with_context(|| {
                    format!(
                        "repo {}: error updating segmented changelog version store",
                        self.repo_id
                    )
                })?;
            info!(
                ctx.logger(),
                "repo {}: successful incremental update to segmented changelog", self.repo_id,
            );
        }

        // Update IdMapVersion
        self.idmap_version_store
            .set(&ctx, idmap_version)
            .await
            .context("updating idmap version")?;

        let owned = OwnedSegmentedChangelog::new(iddag, idmap);
        Ok(owned)
    }
}
