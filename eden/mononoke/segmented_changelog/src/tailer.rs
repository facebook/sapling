/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use slog::debug;
use slog::error;
use slog::info;
use sql_ext::facebook::MyAdmin;
use sql_ext::facebook::MysqlOptions;
use sql_ext::replication::NoReplicaLagMonitor;
use sql_ext::replication::ReplicaLagMonitor;

use stats::prelude::*;

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore_factory::make_metadata_sql_factory;
use blobstore_factory::ReadOnlyStorage;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bookmarks::Bookmarks;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use changeset_fetcher::ChangesetFetcher;
use changeset_fetcher::PrefetchedChangesetsFetcher;
use changesets::ChangesetEntry;
use changesets::ChangesetsArc;
use context::CoreContext;
use metaconfig_types::MetadataDatabaseConfig;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use phases::PhasesArc;
use tunables::tunables;

use crate::dag::ops::DagAddHeads;
use crate::dag::DagAlgorithm;
use crate::iddag::IdDagSaveStore;
use crate::idmap::cs_id_from_vertex_name;
use crate::idmap::CacheHandlers;
use crate::idmap::IdMapFactory;
use crate::owned::OwnedSegmentedChangelog;
use crate::parents::FetchParents;
use crate::types::IdMapVersion;
use crate::types::SegmentedChangelogVersion;
use crate::update::server_namedag;
use crate::update::vertexlist_from_seedheads;
use crate::update::SeedHead;
use crate::version_store::SegmentedChangelogVersionStore;
use crate::CloneHints;
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

const DEFAULT_LOG_SAMPLING_RATE: usize = 5000;

pub struct SegmentedChangelogTailer {
    repo_id: RepositoryId,
    changeset_fetcher: Arc<PrefetchedChangesetsFetcher>,
    bulk_fetch: Arc<PublicChangesetBulkFetch>,
    bookmarks: Arc<dyn Bookmarks>,
    seed_heads: Vec<SeedHead>,
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
    clone_hints: CloneHints,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
}

impl SegmentedChangelogTailer {
    pub fn new(
        repo_id: RepositoryId,
        connections: SegmentedChangelogSqlConnections,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        changeset_fetcher: Arc<PrefetchedChangesetsFetcher>,
        bulk_fetch: Arc<PublicChangesetBulkFetch>,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        blobstore: Arc<dyn Blobstore>,
        bookmarks: Arc<dyn Bookmarks>,
        seed_heads: Vec<SeedHead>,
        caching: Option<(FacebookInit, cachelib::VolatileLruCachePool)>,
    ) -> Self {
        let clone_hints = CloneHints::new(connections.0.clone(), repo_id, blobstore.clone());
        let sc_version_store = SegmentedChangelogVersionStore::new(connections.0.clone(), repo_id);
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
            clone_hints,
            bonsai_hg_mapping,
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
        .with_context(|| format!("constructing metadata sql factory for repo {}", repo_id))?;

        let segmented_changelog_sql_connections = sql_factory
            .open::<SegmentedChangelogSqlConnections>()
            .with_context(|| {
                format!(
                    "error constructing segmented changelog sql connections for repo {}",
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

        let bonsai_hg_mapping = blobrepo.bonsai_hg_mapping_arc();

        Ok(SegmentedChangelogTailer::new(
            repo_id,
            segmented_changelog_sql_connections,
            replica_lag_monitor,
            changeset_fetcher,
            bulk_fetcher,
            bonsai_hg_mapping,
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
            debug!(ctx.logger(), "woke up to update");

            STATS::count.add_value(1);
            STATS::count_per_repo.add_value(1, (self.repo_id.id(),));

            let (stats, update_result) = self.once(ctx, false).timed().await;

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
                        "failed to incrementally update segmented changelog: {:?}", err
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
            "starting incremental update to segmented changelog",
        );

        let (seeding, idmap_version, iddag) = {
            let sc_version = self.sc_version_store.get(ctx).await.with_context(|| {
                format!(
                    "error loading segmented changelog version for repo {}",
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
                            .load(ctx, sc_version.iddag_version)
                            .await
                            .with_context(|| {
                                format!("failed to load iddag for repo {}", self.repo_id)
                            })?;
                        (false, sc_version.idmap_version, iddag)
                    }
                }
                None => (true, IdMapVersion(1), InProcessIdDag::new_in_process()),
            }
        };

        if let Ok(set) = iddag.all() {
            info!(
                ctx.logger(),
                "iddag initialized, it covers {} ids",
                set.count(),
            );
        }
        let idmap = self.idmap_factory.for_writer(ctx, idmap_version);

        let mut namedag = server_namedag(ctx.clone(), iddag, idmap)?;

        let heads =
            vertexlist_from_seedheads(ctx, &self.seed_heads, self.bookmarks.as_ref()).await?;

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
                let repo_bounds = self
                    .bulk_fetch
                    .get_repo_bounds_after_commits(ctx, head_commits)
                    .await?;
                info!(ctx.logger(), "prefetching changeset entries",);
                let mut counter = 0usize;
                // This has the potential to cause OOM by fetching a large
                // chunk of the repo
                let missing = self
                    .bulk_fetch
                    .fetch_bounded(ctx, Direction::NewestFirst, Some(repo_bounds))
                    .map(|res| {
                        counter += 1;
                        let sampling_rate =
                            tunables().get_segmented_changelog_tailer_log_sampling_rate();
                        let sampling_rate = if sampling_rate <= 0 {
                            DEFAULT_LOG_SAMPLING_RATE
                        } else {
                            sampling_rate as usize
                        };
                        if counter % sampling_rate == 0 {
                            info!(
                                ctx.logger(),
                                "fetched {} changeset entries in total", counter,
                            );
                        }
                        res
                    });
                Arc::new(self.changeset_fetcher.clone_with_extension(missing).await?)
            } else {
                self.changeset_fetcher.clone()
            }
        };

        let parent_fetcher = FetchParents::new(ctx.clone(), changeset_fetcher);

        info!(ctx.logger(), "starting the actual update");
        // Note on memory use: we do not flush the changes out in the middle
        // of writing to the IdMap.
        // Thus, if OOMs happen here, the IdMap may need to flush writes to the DB
        // at interesting points.
        let changed = namedag.add_heads(&parent_fetcher, &heads).await?;

        self.clone_hints
            .add_hints(
                ctx,
                &namedag,
                idmap_version,
                self.bonsai_hg_mapping.as_ref(),
            )
            .await?;

        let (idmap, iddag) = namedag.into_idmap_dag();
        let idmap = idmap.finish().await?;

        if !changed {
            info!(
                ctx.logger(),
                "segmented changelog already up to date, skipping update to iddag",
            );
            let owned = OwnedSegmentedChangelog::new(iddag, idmap);
            return Ok(owned);
        }

        info!(ctx.logger(), "IdMap updated, IdDag updated",);

        // Save the IdDag
        let iddag_version = self
            .iddag_save_store
            .save(ctx, &iddag)
            .await
            .with_context(|| format!("error saving iddag for repo {}", self.repo_id))?;

        // Update SegmentedChangelogVersion
        let sc_version = SegmentedChangelogVersion::new(iddag_version, idmap_version);
        if seeding {
            self.sc_version_store
                .set(ctx, sc_version)
                .await
                .with_context(|| {
                    format!(
                        "error updating segmented changelog version store for repo {}",
                        self.repo_id
                    )
                })?;
            info!(ctx.logger(), "successfully seeded segmented changelog",);
        } else {
            self.sc_version_store
                .update(ctx, sc_version)
                .await
                .with_context(|| {
                    format!(
                        "error updating segmented changelog version store for repo {}",
                        self.repo_id
                    )
                })?;
            info!(
                ctx.logger(),
                "successful incremental update to segmented changelog",
            );
        }

        let owned = OwnedSegmentedChangelog::new(iddag, idmap);
        Ok(owned)
    }
}
