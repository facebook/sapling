/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(let_chains)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::RangeBounds;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

#[cfg(fbcode_build)]
use MononokeWarmBookmarkCacheStats_ods3::Instrument_MononokeWarmBookmarkCacheStats;
#[cfg(fbcode_build)]
use MononokeWarmBookmarkCacheStats_ods3_types::MononokeWarmBookmarkCacheStats;
#[cfg(fbcode_build)]
use MononokeWarmBookmarkCacheStats_ods3_types::WarmBookmarkCacheEvent;
use anyhow::Context as _;
use anyhow::Error;
use async_trait::async_trait;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blame::RootBlameV2;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use bookmarks::BookmarksSubscription;
use bookmarks::Freshness;
use bookmarks_cache::BookmarksCache;
use bookmarks_types::Bookmark;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkPagination;
use bookmarks_types::BookmarkPrefix;
use case_conflict_skeleton_manifest::RootCaseConflictSkeletonManifestId;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use context::SessionClass;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use fastlog::RootFastlog;
use filenodes_derivation::FilenodesOnlyPublic;
use fsnodes::RootFsnodeId;
use futures::channel::oneshot;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::select;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use futures_watchdog::WatchdogExt;
use git_types::MappedGitCommitId;
use git_types::RootGitDeltaManifestV2Id;
use inferred_copy_from::RootInferredCopyFromId;
use itertools::Itertools;
#[cfg(fbcode_build)]
use lazy_static::lazy_static;
use lock_ext::RwLockExt;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::Timestamp;
use phases::ArcPhases;
use repo_derived_data::ArcRepoDerivedData;
use repo_event_publisher::ArcRepoEventPublisher;
use repo_event_publisher::RepoEventPublisher;
use repo_identity::ArcRepoIdentity;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use skeleton_manifest::RootSkeletonManifestId;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;
use stats::prelude::*;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::Instrument;
use unodes::RootUnodeManifestId;

mod warmers;
pub use warmers::create_derived_data_warmer;
pub use warmers::create_public_phase_warmer;

#[cfg(fbcode_build)]
lazy_static! {
    static ref WBC_INSTRUMENT: Instrument_MononokeWarmBookmarkCacheStats =
        Instrument_MononokeWarmBookmarkCacheStats::new();
}

define_stats! {
    prefix = "mononoke.warm_bookmarks_cache";
    max_staleness_secs: dynamic_singleton_counter("{}.max_staleness_secs", (reponame: String)),
    global_max_staleness_secs: histogram(10, 0, 5000, Average; P 50; P 75; P 95; P 99),
}

pub struct WarmBookmarksCache {
    bookmarks: Arc<RwLock<HashMap<BookmarkKey, (ChangesetId, BookmarkKind)>>>,
    terminate: Option<oneshot::Sender<()>>,
    notify_sync_start: Arc<Notify>,
    notify_sync_complete: Arc<Notify>,
}

pub type WarmerFn =
    dyn for<'a> Fn(&'a CoreContext, ChangesetId) -> BoxFuture<'a, Result<(), Error>> + Send + Sync;

pub type IsWarmFn = dyn for<'a> Fn(&'a CoreContext, ChangesetId) -> BoxFuture<'a, Result<bool, Error>>
    + Send
    + Sync;

pub struct Warmer {
    warmer: Box<WarmerFn>,
    is_warm: Box<IsWarmFn>,
    name: String,
}

/// Initialization mode for the warm bookmarks cache.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum InitMode {
    /// Rewind each bookmark until a warm changeset is found.
    Rewind,

    /// Warm up the bookmark at its current location.
    Warm,
}

pub struct WarmBookmarksCacheBuilder {
    ctx: CoreContext,
    bookmarks: ArcBookmarks,
    bookmark_update_log: ArcBookmarkUpdateLog,
    repo_identity: ArcRepoIdentity,
    repo_event_publisher: ArcRepoEventPublisher,
    warmers: Vec<Warmer>,
    init_mode: InitMode,
}

impl WarmBookmarksCacheBuilder {
    pub fn new(
        mut ctx: CoreContext,
        bookmarks: ArcBookmarks,
        bookmark_update_log: ArcBookmarkUpdateLog,
        repo_identity: ArcRepoIdentity,
        repo_event_publisher: ArcRepoEventPublisher,
    ) -> Self {
        ctx.session_mut()
            .override_session_class(SessionClass::WarmBookmarksCache);
        let ctx = ctx.with_mutated_scuba(|mut scuba_sample_builder| {
            scuba_sample_builder.add("repo", repo_identity.name());
            scuba_sample_builder.add_common_server_data();
            scuba_sample_builder
        });

        Self {
            ctx,
            bookmarks,
            bookmark_update_log,
            repo_identity,
            repo_event_publisher,
            warmers: vec![],
            init_mode: InitMode::Rewind,
        }
    }

    pub fn add_all_warmers(
        &mut self,
        repo_derived_data: &ArcRepoDerivedData,
        phases: &ArcPhases,
    ) -> Result<(), Error> {
        self.add_derived_data_warmers(&repo_derived_data.active_config().types, repo_derived_data)?;
        self.add_public_phase_warmer(phases);
        Ok(())
    }

    pub fn add_hg_warmers(
        &mut self,
        repo_derived_data: &ArcRepoDerivedData,
        phases: &ArcPhases,
    ) -> Result<(), Error> {
        self.add_derived_data_warmers(
            &[
                MappedHgChangesetId::VARIANT,
                FilenodesOnlyPublic::VARIANT,
                RootHgAugmentedManifestId::VARIANT,
            ],
            repo_derived_data,
        )?;
        self.add_public_phase_warmer(phases);
        Ok(())
    }

    pub fn add_git_warmers(
        &mut self,
        repo_derived_data: &ArcRepoDerivedData,
        phases: &ArcPhases,
    ) -> Result<(), Error> {
        self.add_derived_data_warmers(
            &[
                MappedGitCommitId::VARIANT,
                RootGitDeltaManifestV2Id::VARIANT,
            ],
            repo_derived_data,
        )?;
        self.add_public_phase_warmer(phases);
        Ok(())
    }

    pub fn add_specific_types_warmers(
        &mut self,
        repo_derived_data: &ArcRepoDerivedData,
        types: &[DerivableType],
        phases: &ArcPhases,
    ) -> Result<(), Error> {
        self.add_derived_data_warmers(types, repo_derived_data)?;
        self.add_public_phase_warmer(phases);
        Ok(())
    }

    fn add_derived_data_warmers<'a>(
        &mut self,
        types: impl IntoIterator<Item = &'a DerivableType>,
        repo_derived_data: &ArcRepoDerivedData,
    ) -> Result<(), Error> {
        let types = types.into_iter().collect::<HashSet<_>>();

        let config = repo_derived_data.config();
        for ty in types.iter() {
            if config.is_enabled(**ty) {
                self.warmers
                    .extend(self.derived_data_warmer(ty, repo_derived_data));
            }
        }

        Ok(())
    }

    fn derived_data_warmer(
        &self,
        derivable_type: &DerivableType,
        repo_derived_data: &ArcRepoDerivedData,
    ) -> Option<Warmer> {
        match derivable_type {
            DerivableType::Unodes => Some(create_derived_data_warmer::<RootUnodeManifestId>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::BlameV2 => Some(create_derived_data_warmer::<RootBlameV2>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::FileNodes => {
                // TODO: add warmer for filenodes
                None
            }
            DerivableType::HgChangesets => Some(create_derived_data_warmer::<MappedHgChangesetId>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::HgAugmentedManifests => Some(create_derived_data_warmer::<
                RootHgAugmentedManifestId,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::Fsnodes => Some(create_derived_data_warmer::<RootFsnodeId>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::Fastlog => Some(create_derived_data_warmer::<RootFastlog>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::DeletedManifests => Some(create_derived_data_warmer::<
                RootDeletedManifestV2Id,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::SkeletonManifests => Some(create_derived_data_warmer::<
                RootSkeletonManifestId,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::SkeletonManifestsV2 => Some(create_derived_data_warmer::<
                RootSkeletonManifestV2Id,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::Ccsm => Some(create_derived_data_warmer::<
                RootCaseConflictSkeletonManifestId,
            >(&self.ctx, repo_derived_data.clone())),
            DerivableType::ContentManifests => Some(create_derived_data_warmer::<
                RootContentManifestId,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::ChangesetInfo => Some(create_derived_data_warmer::<ChangesetInfo>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::GitDeltaManifestsV2 => Some(create_derived_data_warmer::<
                RootGitDeltaManifestV2Id,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::GitDeltaManifestsV3 => None,
            DerivableType::BssmV3 => Some(create_derived_data_warmer::<RootBssmV3DirectoryId>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::GitCommits => Some(create_derived_data_warmer::<MappedGitCommitId>(
                &self.ctx,
                repo_derived_data.clone(),
            )),
            DerivableType::InferredCopyFrom => Some(create_derived_data_warmer::<
                RootInferredCopyFromId,
            >(
                &self.ctx, repo_derived_data.clone()
            )),
            DerivableType::TestManifests => None,
            DerivableType::TestShardedManifests => None,
        }
    }

    fn add_public_phase_warmer(&mut self, phases: &ArcPhases) {
        let warmer = create_public_phase_warmer(&self.ctx, phases.clone());
        self.warmers.push(warmer);
    }

    /// For use in tests to avoid having to wait for the warm bookmark cache
    /// to finish its initial warming cycle.  When initializing the warm
    /// bookmarks cache, wait until all current bookmark values have been
    /// warmed before returning.
    pub fn wait_until_warmed(&mut self) {
        self.init_mode = InitMode::Warm;
    }

    pub async fn build(self) -> Result<WarmBookmarksCache, Error> {
        WarmBookmarksCache::new(
            &self.ctx,
            &self.bookmarks,
            &self.bookmark_update_log,
            &self.repo_identity,
            &self.repo_event_publisher,
            self.warmers,
            self.init_mode,
        )
        .await
    }
}

/// A drop-in replacement for warm bookmark cache that doesn't
/// cache anything but goes to bookmarks struct every time.
pub struct NoopBookmarksCache {
    bookmarks: ArcBookmarks,
}

impl NoopBookmarksCache {
    pub fn new(bookmarks: ArcBookmarks) -> Self {
        NoopBookmarksCache { bookmarks }
    }
}

#[async_trait]
impl BookmarksCache for NoopBookmarksCache {
    async fn get(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkKey,
    ) -> Result<Option<ChangesetId>, Error> {
        self.bookmarks
            .get(ctx.clone(), bookmark, bookmarks::Freshness::MostRecent)
            .await
    }

    async fn list(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>, Error> {
        self.bookmarks
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                prefix,
                BookmarkCategory::ALL,
                BookmarkKind::ALL_PUBLISHING,
                pagination,
                limit.unwrap_or(u64::MAX),
            )
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_key(), (cs_id, kind))
            })
            .try_collect()
            .await
    }

    async fn sync(&self, _ctx: &CoreContext) {}
}

impl WarmBookmarksCache {
    pub async fn new(
        ctx: &CoreContext,
        bookmarks: &ArcBookmarks,
        bookmark_update_log: &ArcBookmarkUpdateLog,
        repo_identity: &ArcRepoIdentity,
        repo_event_publisher: &ArcRepoEventPublisher,
        warmers: Vec<Warmer>,
        init_mode: InitMode,
    ) -> Result<Self, Error> {
        let warmers = Arc::new(warmers);
        let (sender, receiver) = oneshot::channel();
        let notify_sync_start = Arc::new(Notify::new());
        let notify_sync_complete = Arc::new(Notify::new());

        tracing::info!(repo = %repo_identity.name(), "Starting warm bookmark cache updater");
        let sub = bookmarks
            .create_subscription(ctx, Freshness::MaybeStale)
            .await
            .context("Error creating bookmarks subscription")?;

        let bookmarks_to_watch = init_bookmarks(
            ctx,
            &*sub,
            bookmarks.as_ref(),
            bookmark_update_log.as_ref(),
            &warmers,
            init_mode,
        )
        .instrument(tracing::info_span!("init bookmarks", repo = %repo_identity.name()))
        .await?;

        let bookmarks_to_watch = Arc::new(RwLock::new(bookmarks_to_watch));

        BookmarksCoordinator::new(
            bookmarks_to_watch.clone(),
            sub,
            bookmarks.clone(),
            bookmark_update_log.clone(),
            repo_identity.clone(),
            repo_event_publisher.clone(),
            warmers.clone(),
        )
        .spawn(
            ctx.clone(),
            receiver,
            notify_sync_start.clone(),
            notify_sync_complete.clone(),
        );

        Ok(Self {
            bookmarks: bookmarks_to_watch,
            terminate: Some(sender),
            notify_sync_start,
            notify_sync_complete,
        })
    }
}

#[async_trait]
impl BookmarksCache for WarmBookmarksCache {
    async fn get(
        &self,
        _ctx: &CoreContext,
        bookmark: &BookmarkKey,
    ) -> Result<Option<ChangesetId>, Error> {
        Ok(self
            .bookmarks
            .read()
            .unwrap()
            .get(bookmark)
            .map(|(cs_id, _)| cs_id)
            .cloned())
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>, Error> {
        let bookmarks = self.bookmarks.read().unwrap();

        if prefix.is_empty() && *pagination == BookmarkPagination::FromStart && limit.is_none() {
            // Simple case: return all bookmarks
            Ok(bookmarks
                .iter()
                .map(|(key, (cs_id, kind))| (key.clone(), (*cs_id, *kind)))
                .collect())
        } else {
            // Filter based on prefix and pagination
            let range = prefix.to_range().with_pagination(pagination.clone());
            let mut matches = bookmarks
                .iter()
                .filter(|(key, _)| range.contains(key))
                .map(|(key, (cs_id, kind))| (key.clone(), (*cs_id, *kind)))
                .collect::<Vec<_>>();
            // Release the read lock.
            drop(bookmarks);
            if let Some(limit) = limit {
                // We must sort and truncate if there is a limit so that the
                // client can paginate in order.
                matches.sort_by(|(name1, _), (name2, _)| name1.cmp(name2));
                matches.truncate(limit as usize);
            }
            Ok(matches)
        }
    }

    async fn sync(&self, _ctx: &CoreContext) {
        // Notifies the bookmark coordinator of sync start and
        // starts listening for a notification indicating sync completion.
        let notified = self.notify_sync_complete.notified();
        self.notify_sync_start.notify_one();
        notified.await;
    }
}

impl Drop for WarmBookmarksCache {
    fn drop(&mut self) {
        // Ignore any error - we don't care if the updater has gone away.
        if let Some(terminate) = self.terminate.take() {
            let _ = terminate.send(());
        }
    }
}

async fn init_bookmarks(
    ctx: &CoreContext,
    sub: &dyn BookmarksSubscription,
    bookmarks: &dyn Bookmarks,
    bookmark_update_log: &dyn BookmarkUpdateLog,
    warmers: &Arc<Vec<Warmer>>,
    mode: InitMode,
) -> Result<HashMap<BookmarkKey, (ChangesetId, BookmarkKind)>, Error> {
    let all_bookmarks = sub.bookmarks();
    let total = all_bookmarks.len();

    tracing::info!("{} bookmarks to warm up", total);

    let futs = all_bookmarks
        .iter()
        .enumerate()
        .map(|(i, (book, (cs_id, kind)))| async move {
            let book = book.clone();
            let kind = *kind;
            let cs_id = *cs_id;

            let remaining = total - i - 1;

            if !is_warm(ctx, cs_id, warmers).watched(ctx.logger()).await {
                match mode {
                    InitMode::Rewind => {
                        let maybe_cs_id = move_bookmark_back_in_history_until_derived(
                            ctx,
                            bookmarks,
                            bookmark_update_log,
                            &book,
                            warmers,
                        )
                        .watched(ctx.logger())
                        .await?;

                        tracing::info!("moved {} back in history to {:?}", book, maybe_cs_id);
                        Ok((remaining, maybe_cs_id.map(|cs_id| (book, (cs_id, kind)))))
                    }
                    InitMode::Warm => {
                        tracing::info!("warmed bookmark {} at {}", book, cs_id);
                        warm_all(ctx, cs_id, warmers).watched(ctx.logger()).await?;
                        Ok((remaining, Some((book, (cs_id, kind)))))
                    }
                }
            } else {
                Ok((remaining, Some((book, (cs_id, kind)))))
            }
        })
        .collect::<Vec<_>>(); // This should be unnecessary but it de-confuses the compiler: P415515287.

    let res = stream::iter(futs)
        .buffered(100)
        .try_filter_map(|element| async move {
            let (remaining, entry) = element;
            if remaining % 1000 == 0 {
                tracing::info!("{} bookmarks left to warm up", remaining);
            }
            Result::<_, Error>::Ok(entry)
        })
        .try_collect::<HashMap<_, _>>()
        .await
        .with_context(|| "Error warming up bookmarks")?;

    tracing::info!("all bookmarks are warmed up");

    Ok(res)
}

async fn is_warm(ctx: &CoreContext, cs_id: ChangesetId, warmers: &[Warmer]) -> bool {
    let is_warm = warmers
        .iter()
        .map(|warmer| (*warmer.is_warm)(ctx, cs_id).map(|res| res.unwrap_or(false)))
        .collect::<FuturesUnordered<_>>();

    is_warm
        .fold(true, |acc, is_warm| future::ready(acc & is_warm))
        .await
}

async fn warm_all(ctx: &CoreContext, cs_id: ChangesetId, warmers: &[Warmer]) -> Result<(), Error> {
    stream::iter(warmers.iter().map(Ok))
        .try_for_each_concurrent(100, |warmer| async {
            let (stats, res) = (*warmer.warmer)(ctx, cs_id).timed().await;
            let mut scuba = ctx.scuba().clone();
            scuba
                .add("Warmer name", warmer.name.clone())
                .add_future_stats(&stats);
            match &res {
                Ok(()) => {
                    scuba.log_with_msg("Warmer succeed", None);
                }
                Err(err) => {
                    scuba.log_with_msg("Warmer failed", Some(format!("{:#}", err)));
                }
            }
            res
        })
        .await
}

async fn move_bookmark_back_in_history_until_derived(
    ctx: &CoreContext,
    bookmarks: &dyn Bookmarks,
    bookmark_update_log: &dyn BookmarkUpdateLog,
    book: &BookmarkKey,
    warmers: &Arc<Vec<Warmer>>,
) -> Result<Option<ChangesetId>, Error> {
    tracing::info!("moving {} bookmark back in history...", book);

    let (latest_derived_entry, _) =
        find_latest_derived_and_underived(ctx, bookmarks, bookmark_update_log, book, warmers)
            .await?;

    match latest_derived_entry {
        LatestDerivedBookmarkEntry::Found(maybe_cs_id_and_ts) => {
            Ok(maybe_cs_id_and_ts.map(|(cs_id, _)| cs_id))
        }
        LatestDerivedBookmarkEntry::NotFound => {
            let cur_bookmark_value = bookmarks
                .get(ctx.clone(), book, bookmarks::Freshness::MostRecent)
                .await?;
            tracing::warn!(
                "cannot find previous derived version of {}, returning current version {:?}",
                book,
                cur_bookmark_value
            );
            Ok(cur_bookmark_value)
        }
    }
}

pub enum LatestDerivedBookmarkEntry {
    /// Timestamp can be None if no history entries are found
    Found(Option<(ChangesetId, Option<Timestamp>)>),
    /// Latest derived bookmark entry is too far away
    NotFound,
}

#[derive(Default)]
pub struct LatestUnderivedBookmarkEntry {
    /// Changeset ID of the latest underived bookmark entry.  This is the next
    /// thing to try to derive.
    maybe_cs_id: Option<ChangesetId>,
    /// ID and TS for the oldest underived bookmark entry for logging
    maybe_id_ts: Option<(BookmarkUpdateLogId, Timestamp)>,
}

/// Searches bookmark log for latest entry for which everything is derived. Note that we consider log entry that
/// deletes a bookmark to be derived.
pub async fn find_latest_derived_and_underived(
    ctx: &CoreContext,
    bookmarks: &dyn Bookmarks,
    bookmark_update_log: &dyn BookmarkUpdateLog,
    book: &BookmarkKey,
    warmers: &[Warmer],
) -> Result<(LatestDerivedBookmarkEntry, LatestUnderivedBookmarkEntry), Error> {
    let mut latest_underived = LatestUnderivedBookmarkEntry::default();
    let mut found_latest_entry = false;
    let history_depth_limits = vec![0, 10, 50, 100, 1000, 10000];

    for (prev_limit, limit) in history_depth_limits.into_iter().tuple_windows() {
        if prev_limit > 0 {
            tracing::debug!("{} bookmark, limit {}", book, limit);
        }
        // Note that since new entries might be inserted to the bookmark log,
        // the next call to `list_bookmark_log_entries(...)` might return
        // entries that were already returned on the previous call `list_bookmark_log_entries(...)`.
        // That means that the same entry might be rechecked, but that shouldn't be a big problem.
        let mut log_entries = bookmark_update_log
            .list_bookmark_log_entries(
                ctx.clone(),
                book.clone(),
                limit,
                Some(prev_limit),
                Freshness::MaybeStale,
            )
            .map_ok(|(id, maybe_cs_id, _, ts)| {
                let id = id.into();
                (maybe_cs_id, Some((id, ts)))
            })
            .try_collect::<Vec<_>>()
            .await?;

        if log_entries.is_empty() {
            tracing::debug!("bookmark {} has no history in the log", book);
            let maybe_cs_id = bookmarks
                .get(ctx.clone(), book, bookmarks::Freshness::MostRecent)
                .await?;
            // If a bookmark has no history then we add a fake entry saying that
            // timestamp is unknown.
            log_entries.push((maybe_cs_id, None));
        }

        let log_entries_fetched = log_entries.len();
        if !found_latest_entry {
            if let Some((maybe_cs_id, _)) = log_entries.first() {
                latest_underived.maybe_cs_id = *maybe_cs_id;
                found_latest_entry = true;
            }
        }

        let mut maybe_derived = stream::iter(log_entries.into_iter().map(
            |(maybe_cs_id, id_and_ts)| async move {
                match maybe_cs_id {
                    Some(cs_id) => {
                        let derived = is_warm(ctx, cs_id, warmers).await;
                        (Some((cs_id, id_and_ts)), derived)
                    }
                    None => (None, true),
                }
            },
        ))
        .buffered(100);

        while let Some((maybe_cs_id_ts, is_warm)) = maybe_derived.next().watched(ctx.logger()).await
        {
            if is_warm {
                // Remove bookmark update log id
                let maybe_cs_ts =
                    maybe_cs_id_ts.map(|(cs_id, id_and_ts)| (cs_id, id_and_ts.map(|(_, ts)| ts)));
                return Ok((
                    LatestDerivedBookmarkEntry::Found(maybe_cs_ts),
                    latest_underived,
                ));
            } else {
                // Store the oldest underived bookmarke entry ID and ts for logging purpose
                latest_underived.maybe_id_ts = maybe_cs_id_ts.and_then(|(_, id_and_ts)| id_and_ts);
            }
        }

        // Bookmark has been created recently and wasn't derived at all
        if (log_entries_fetched as u32) < limit {
            return Ok((LatestDerivedBookmarkEntry::Found(None), latest_underived));
        }
    }

    Ok((LatestDerivedBookmarkEntry::NotFound, latest_underived))
}

#[facet::container]
#[derive(Clone)]
struct BookmarksCoordinatorRepo {
    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_event_publisher: dyn RepoEventPublisher,
}

struct BookmarksCoordinator {
    bookmarks: Arc<RwLock<HashMap<BookmarkKey, (ChangesetId, BookmarkKind)>>>,
    sub: Box<dyn BookmarksSubscription>,
    repo: BookmarksCoordinatorRepo,
    warmers: Arc<Vec<Warmer>>,
    live_updaters: Arc<RwLock<HashMap<BookmarkKey, BookmarkUpdaterState>>>,
    updaters_handles: HashMap<BookmarkKey, JoinHandle<()>>,
}

impl BookmarksCoordinator {
    fn new(
        bookmarks: Arc<RwLock<HashMap<BookmarkKey, (ChangesetId, BookmarkKind)>>>,
        sub: Box<dyn BookmarksSubscription>,
        bookmarks_fetcher: ArcBookmarks,
        bookmark_update_log: ArcBookmarkUpdateLog,
        repo_identity: ArcRepoIdentity,
        repo_event_publisher: ArcRepoEventPublisher,
        warmers: Arc<Vec<Warmer>>,
    ) -> Self {
        let repo = BookmarksCoordinatorRepo {
            bookmarks: bookmarks_fetcher,
            bookmark_update_log,
            repo_identity,
            repo_event_publisher,
        };

        Self {
            bookmarks,
            sub,
            repo,
            warmers,
            live_updaters: Arc::new(RwLock::new(HashMap::new())),
            updaters_handles: Default::default(),
        }
    }

    async fn update(&mut self, ctx: &CoreContext) -> Result<(), Error> {
        report_delay_and_remove_finished_updaters(
            ctx,
            &self.live_updaters,
            self.repo.repo_identity().name(),
        );

        let cur_bookmarks = self.bookmarks.with_read(|bookmarks| bookmarks.clone());

        self.sub
            .refresh(ctx)
            .await
            .context("Error refreshing subscription")?;

        let new_bookmarks = Cow::Borrowed(self.sub.bookmarks());

        let mut changed_bookmarks = vec![];
        // Find bookmarks that were moved/created and spawn an updater
        // for them
        for (key, new_value) in new_bookmarks.iter() {
            let cur_value = cur_bookmarks.get(key);
            if Some(new_value) != cur_value {
                let book = Bookmark::new(key.clone(), new_value.1);
                changed_bookmarks.push(book);
            }
        }

        // Find bookmarks that were deleted
        for (key, cur_value) in &cur_bookmarks {
            // There's a potential race condition if a bookmark was deleted
            // and then immediately recreated with another kind (e.g. Publishing instead of
            // PullDefault). Because of the race WarmBookmarksCache might store a
            // bookmark with an old Kind.
            // I think this is acceptable because it's going to be fixed on next iteration
            // of the loop, and because fixing it properly is hard if not impossible -
            // change of bookmark kind is not reflected in update log.
            if !new_bookmarks.contains_key(key) {
                let book = Bookmark::new(key.clone(), cur_value.1);
                changed_bookmarks.push(book);
            }
        }

        for book in changed_bookmarks {
            let need_spawning = self.live_updaters.with_write(|live_updaters| {
                if !live_updaters.contains_key(book.key()) {
                    live_updaters.insert(book.key().clone(), BookmarkUpdaterState::Started);
                    true
                } else {
                    false
                }
            });
            // It's possible that bookmark updater removes a bookmark right after
            // we tried to insert it. That means we won't spawn a new updater,
            // but that's not a big deal though - we'll do it on the next iteration
            // of the loop
            if need_spawning {
                cloned!(
                    ctx,
                    self.repo,
                    self.bookmarks,
                    self.live_updaters,
                    self.warmers,
                );
                self.updaters_handles.insert(
                    book.key().clone(),
                    mononoke::spawn_task(async move {
                        let res = single_bookmark_updater(
                            &ctx,
                            &repo,
                            &book,
                            &bookmarks,
                            &warmers,
                            |ts: Timestamp| {
                                live_updaters.with_write(|live_updaters| {
                                    live_updaters.insert(
                                        book.key().clone(),
                                        BookmarkUpdaterState::InProgress {
                                            oldest_underived_ts: ts,
                                        },
                                    );
                                });
                            },
                        )
                        .await;
                        if let Err(ref err) = res {
                            #[cfg(fbcode_build)]
                            WBC_INSTRUMENT.observe(MononokeWarmBookmarkCacheStats {
                                event: Some(WarmBookmarkCacheEvent::UpdateFailure),
                                ..Default::default()
                            });
                            tracing::warn!("update of {} failed: {:?}", book.key(), err);
                        };

                        live_updaters.with_write(|live_updaters| {
                            let maybe_state = live_updaters.remove(book.key());
                            if let Some(state) = maybe_state {
                                live_updaters.insert(book.key().clone(), state.into_finished(&res));
                            }
                        });
                    }),
                );
            }
        }

        Ok(())
    }

    // Loop that finds bookmarks that were modified and spawns separate bookmark updaters for them
    pub fn spawn(
        mut self,
        ctx: CoreContext,
        terminate: oneshot::Receiver<()>,
        notify_sync_start: Arc<Notify>,
        notify_sync_complete: Arc<Notify>,
    ) {
        let span = tracing::info_span!("wbc", repo = %self.repo.repo_identity().name());
        let fut = async move {
            tracing::info!("Started warm bookmark cache updater");
            let repo_name: String = self.repo.repo_identity().name().to_string();
            let tailing_enabled = justknobs::eval(
                "scm/mononoke:wbc_update_by_scribe_tailer",
                None,
                Some(&repo_name),
            )
            .unwrap_or(false);
            let mut bookmark_update_subscriber = tailing_enabled
                .then(|| {
                    self.repo
                        .repo_event_publisher
                        .subscribe_for_bookmark_updates(&repo_name)
                        .ok()
                })
                .flatten();
            let infinite_loop = async {
                // Indicates that the sync method was called and is waiting for a sync
                // completion notification
                let mut sync_started = false;
                loop {
                    // Reset the sequence counter for each loop iteration.
                    let ctx = ctx.with_mutated_scuba(|scuba| scuba.with_seq("seq"));
                    let res = self.update(&ctx).await;

                    if let Err(err) = res.as_ref() {
                        #[cfg(fbcode_build)]
                        WBC_INSTRUMENT.observe(MononokeWarmBookmarkCacheStats {
                            event: Some(WarmBookmarkCacheEvent::DiscoverFailure),
                            ..Default::default()
                        });
                        tracing::warn!("failed to update bookmarks {:?}", err);
                    }

                    if sync_started {
                        // Wait for all updaters to finish and notify sync completion
                        if let Err(join_err) = self
                            .updaters_handles
                            .drain()
                            .map(|(_, handle)| handle)
                            .collect::<FuturesUnordered<_>>()
                            .try_collect::<Vec<_>>()
                            .await
                        {
                            tracing::warn!(
                                "failed to join updater tasks when syncing {:?}",
                                join_err
                            );
                        }
                        notify_sync_complete.notify_waiters();
                    }

                    const FALLBACK_WBC_POLL_INTERVAL_MS: u64 = 5000;
                    let delay = Duration::from_millis(
                        justknobs::get_as::<u64>(
                            "scm/mononoke:warm_bookmark_cache_poll_interval_ms",
                            None,
                        )
                        .unwrap_or(FALLBACK_WBC_POLL_INTERVAL_MS),
                    );

                    let tailing_enabled = justknobs::eval(
                        "scm/mononoke:wbc_update_by_scribe_tailer",
                        None,
                        Some(&repo_name),
                    )
                    .unwrap_or(false);

                    // Receiving a sync notification interrupts sleep/listen and forces
                    // waiting for all updaters to finish in the next iteration
                    let notified = notify_sync_start.notified();

                    if tailing_enabled && let Some(sub) = bookmark_update_subscriber.as_mut() {
                        let receiver_fut = sub.recv();
                        futures::pin_mut!(notified, receiver_fut);
                        match select(notified, receiver_fut).await {
                            future::Either::Left(_) => sync_started = true,
                            future::Either::Right(_) => sync_started = false,
                        }
                    } else {
                        let sleep = tokio::time::sleep(delay);
                        futures::pin_mut!(notified, sleep);
                        match select(notified, sleep).await {
                            future::Either::Left(_) => sync_started = true,
                            future::Either::Right(_) => sync_started = false,
                        }
                    }
                }
            }
            .boxed();

            let _ = select(infinite_loop, terminate).await;

            tracing::info!("Stopped warm bookmark cache updater");
        }
        .instrument(span);

        // Fire and forget. This will terminate using the `terminate` receiver.
        std::mem::drop(mononoke::spawn_task(fut));
    }
}

fn report_delay_and_remove_finished_updaters(
    ctx: &CoreContext,
    live_updaters: &Arc<RwLock<HashMap<BookmarkKey, BookmarkUpdaterState>>>,
    reponame: &str,
) {
    let mut max_staleness = 0;
    live_updaters.with_write(|live_updaters| {
        let new_updaters = live_updaters
            .drain()
            .filter_map(|(key, value)| {
                use BookmarkUpdaterState::*;
                match value {
                    Started => {}
                    InProgress {
                        oldest_underived_ts,
                    } => {
                        max_staleness =
                            ::std::cmp::max(max_staleness, oldest_underived_ts.since_seconds());
                    }
                    Finished {
                        oldest_underived_ts,
                    } => {
                        if let Some(oldest_underived_ts) = oldest_underived_ts {
                            let staleness_secs = oldest_underived_ts.since_seconds();
                            max_staleness = ::std::cmp::max(max_staleness, staleness_secs);
                        }
                    }
                };

                if value.is_finished() {
                    None
                } else {
                    Some((key, value))
                }
            })
            .collect();
        *live_updaters = new_updaters;
    });

    STATS::max_staleness_secs.set_value(ctx.fb, max_staleness, (reponame.to_owned(),));
    STATS::global_max_staleness_secs.add_value(max_staleness);
    #[cfg(fbcode_build)]
    WBC_INSTRUMENT.observe(MononokeWarmBookmarkCacheStats {
        repo: Some(reponame.to_owned()),
        max_staleness_secs: Some(max_staleness as f64),
        ..Default::default()
    });
}

#[derive(Clone)]
enum BookmarkUpdaterState {
    // Updater has started but it hasn't yet fetched bookmark update log
    Started,
    // Updater is deriving right now
    InProgress {
        oldest_underived_ts: Timestamp,
    },
    // Updater is finished. it might report staleness it it failed.
    Finished {
        oldest_underived_ts: Option<Timestamp>,
    },
}

impl BookmarkUpdaterState {
    fn into_finished(self, bookmark_updater_res: &Result<(), Error>) -> BookmarkUpdaterState {
        use BookmarkUpdaterState::*;

        let oldest_underived_ts = match self {
            // That might happen if by the time updater started everything was already derived
            // or updater failed before it started deriving
            Started => None,
            InProgress {
                oldest_underived_ts,
            } => {
                if bookmark_updater_res.is_err() {
                    Some(oldest_underived_ts)
                } else {
                    None
                }
            }
            // That shouldn't really happen in practice
            // but return anyway
            Finished {
                oldest_underived_ts,
            } => oldest_underived_ts,
        };

        Finished {
            oldest_underived_ts,
        }
    }
}

impl BookmarkUpdaterState {
    fn is_finished(&self) -> bool {
        match self {
            Self::Started | Self::InProgress { .. } => false,
            Self::Finished { .. } => true,
        }
    }
}

async fn single_bookmark_updater(
    ctx: &CoreContext,
    repo: &(impl BookmarksRef + BookmarkUpdateLogRef),
    bookmark: &Bookmark,
    bookmarks: &Arc<RwLock<HashMap<BookmarkKey, (ChangesetId, BookmarkKind)>>>,
    warmers: &Arc<Vec<Warmer>>,
    mut staleness_reporter: impl FnMut(Timestamp),
) -> Result<(), Error> {
    let (latest_derived, latest_underived) = find_latest_derived_and_underived(
        ctx,
        repo.bookmarks(),
        repo.bookmark_update_log(),
        bookmark.key(),
        warmers.as_ref(),
    )
    .await?;

    let update_bookmark = |cs_id: ChangesetId| async move {
        bookmarks.with_write(|bookmarks| {
            let key = bookmark.key().clone();
            bookmarks.insert(key, (cs_id, *bookmark.kind()))
        });
    };

    match latest_derived {
        // Move bookmark to the latest derived commit or delete the bookmark completely
        LatestDerivedBookmarkEntry::Found(maybe_cs_id_and_ts) => match maybe_cs_id_and_ts {
            Some((cs_id, _ts)) => {
                update_bookmark(cs_id).await;
            }
            None => {
                bookmarks.with_write(|bookmarks| bookmarks.remove(bookmark.key()));
            }
        },
        LatestDerivedBookmarkEntry::NotFound => {
            tracing::warn!(
                "Haven't found previous derived version of {}! Will try to derive anyway",
                bookmark.key()
            );
        }
    }

    let LatestUnderivedBookmarkEntry {
        maybe_cs_id,
        maybe_id_ts,
    } = latest_underived;
    if let Some(underived_cs_id) = maybe_cs_id {
        if let Some((_, ts)) = maybe_id_ts {
            // timestamp might not be known if e.g. bookmark has no history.
            // In that case let's not report staleness
            staleness_reporter(ts);
        }

        let bookmark_log_id = maybe_id_ts.as_ref().map(|(id, _)| u64::from(*id));
        let maybe_ts = maybe_id_ts.map(|(_, ts)| ts);

        let ctx = ctx.clone().with_mutated_scuba(|mut scuba| {
            scuba.add("bookmark", bookmark.key().to_string());
            scuba.add("bookmark_log_id", bookmark_log_id);
            scuba.add("top_changeset", underived_cs_id.to_hex().to_string());
            scuba
        });

        ctx.scuba()
            .clone()
            .add("delay_ms", maybe_ts.map(|ts| ts.since_millis()))
            .log_with_msg("Before warming bookmark", None);
        let (stats, res) = warm_all(&ctx, underived_cs_id, warmers).timed().await;
        ctx.scuba()
            .clone()
            .add_future_stats(&stats)
            .log_with_msg("After warming bookmark", None);

        match res {
            Ok(()) => {
                update_bookmark(underived_cs_id).await;
            }
            Err(err) => {
                tracing::warn!(
                    "failed to derive data for {} while updating {}: {}",
                    underived_cs_id,
                    bookmark.key(),
                    err
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::BookmarkUpdateLog;
    use bookmarks::BookmarkUpdateLogArc;
    use bookmarks::Bookmarks;
    use bookmarks::BookmarksArc;
    use bookmarks::BookmarksMaybeStaleExt;
    use cloned::cloned;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use delayblob::DelayedBlobstore;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use maplit::hashmap;
    use memblob::Memblob;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataArc;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_event_publisher::RepoEventPublisher;
    use repo_event_publisher::RepoEventPublisherArc;
    use repo_identity::RepoIdentity;
    use repo_identity::RepoIdentityArc;
    use sql_ext::mononoke_queries;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;
    use tests_utils::resolve_cs_id;
    use tokio::time;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    pub struct Repo(
        RepoIdentity,
        RepoBlobstore,
        RepoDerivedData,
        CommitGraph,
        dyn CommitGraphWriter,
        dyn BonsaiHgMapping,
        dyn BookmarkUpdateLog,
        dyn Bookmarks,
        dyn RepoEventPublisher,
        FilestoreConfig,
    );

    #[mononoke::fbinit_test]
    async fn test_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let sub = repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        // Unodes haven't been derived at all - so we should get an empty set of bookmarks
        let bookmarks = init_bookmarks(
            &ctx,
            &*sub,
            repo.bookmarks(),
            repo.bookmark_update_log(),
            &warmers,
            InitMode::Rewind,
        )
        .await?;
        assert_eq!(bookmarks, HashMap::new());

        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        repo.repo_derived_data()
            .derive::<RootUnodeManifestId>(&ctx, master_cs_id)
            .await?;

        let bookmarks = init_bookmarks(
            &ctx,
            &*sub,
            repo.bookmarks(),
            repo.bookmark_update_log(),
            &warmers,
            InitMode::Rewind,
        )
        .await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkKey::new("master")? => (master_cs_id, BookmarkKind::PullDefaultPublishing)}
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_derived(fb: FacebookInit) -> Result<(), Error> {
        let put_distr = rand_distr::Normal::<f64>::new(0.1, 0.05).unwrap();
        let get_distr = rand_distr::Normal::<f64>::new(0.05, 0.025).unwrap();
        let blobstore = Arc::new(DelayedBlobstore::new(
            Memblob::default(),
            put_distr,
            get_distr,
        ));
        let repo: Repo = TestRepoFactory::new(fb)?
            .with_blobstore(blobstore)
            .build()
            .await?;
        Linear::init_repo(fb, &repo).await?;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        tracing::info!("creating 5 derived commits");
        let mut master = resolve_cs_id(&ctx, &repo, "master").await?;
        for _ in 1..5 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec![master])
                .commit()
                .await?;

            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
            master = new_master;
        }
        repo.repo_derived_data()
            .derive::<RootUnodeManifestId>(&ctx, master)
            .await?;
        let derived_master = master;

        tracing::info!("creating 5 more underived commits");
        for _ in 1..5 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec![master])
                .commit()
                .await?;
            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
            master = new_master;
        }

        let sub = repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let bookmarks = init_bookmarks(
            &ctx,
            &*sub,
            repo.bookmarks(),
            repo.bookmark_update_log(),
            &warmers,
            InitMode::Rewind,
        )
        .await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkKey::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        repo.repo_derived_data()
            .derive::<RootUnodeManifestId>(&ctx, master)
            .await?;
        let bookmarks = init_bookmarks(
            &ctx,
            &*sub,
            repo.bookmarks(),
            repo.bookmark_update_log(),
            &warmers,
            InitMode::Rewind,
        )
        .await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkKey::new("master")? => (master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_a_lot_of_moves(fb: FacebookInit) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        let derived_master = resolve_cs_id(&ctx, &repo, "master").await?;
        repo.repo_derived_data()
            .derive::<RootUnodeManifestId>(&ctx, derived_master)
            .await?;

        for i in 1..50 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
                .add_file(format!("{}", i).as_str(), "content")
                .commit()
                .await?;

            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
        }

        let sub = repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let bookmarks = init_bookmarks(
            &ctx,
            &*sub,
            repo.bookmarks(),
            repo.bookmark_update_log(),
            &warmers,
            InitMode::Rewind,
        )
        .await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkKey::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_derived_right_after_threshold(fb: FacebookInit) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        let derived_master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        repo.repo_derived_data()
            .derive::<RootUnodeManifestId>(&ctx, derived_master)
            .await?;
        bookmark(&ctx, &repo, "master")
            .set_to(derived_master)
            .await?;

        // First history threshold is 10. Let's make sure we don't have off-by one errors
        for i in 0..10 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
                .add_file(format!("{}", i).as_str(), "content")
                .commit()
                .await?;

            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
        }

        let sub = repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let bookmarks = init_bookmarks(
            &ctx,
            &*sub,
            repo.bookmarks(),
            repo.bookmark_update_log(),
            &warmers,
            InitMode::Rewind,
        )
        .await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkKey::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_spawn_bookmarks_coordinator_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .bookmarks()
            .get_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_key(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "master").set_to(master).await?;

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            repo.bookmarks_arc(),
            repo.bookmark_update_log_arc(),
            repo.repo_identity_arc(),
            repo.repo_event_publisher_arc(),
            warmers,
        );

        let master_book = BookmarkKey::new("master")?;
        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &master_book,
            Some((master, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        bookmark(&ctx, &repo, "master").delete().await?;
        // This check should not be successful in deleting master because it is protected
        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &master_book,
            Some((master, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_single_bookmarks_coordinator_many_updates(fb: FacebookInit) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .bookmarks()
            .get_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_key(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        tracing::info!("created stack of commits");
        for i in 1..10 {
            let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
                .add_file(format!("somefile{}", i).as_str(), "content")
                .commit()
                .await?;
            tracing::info!("created {}", master);
            bookmark(&ctx, &repo, "master").set_to(master).await?;
        }
        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        tracing::info!("created the whole stack of commits");

        let master_book_name = BookmarkKey::new("master")?;
        let master_book = Bookmark::new(
            master_book_name.clone(),
            BookmarkKind::PullDefaultPublishing,
        );
        single_bookmark_updater(&ctx, &repo, &master_book, &bookmarks, &warmers, |_| {}).await?;

        assert_eq!(
            bookmarks.with_read(|bookmarks| bookmarks.get(&master_book_name).cloned()),
            Some((master_cs_id, BookmarkKind::PullDefaultPublishing))
        );

        Ok(())
    }

    async fn update_and_wait_for_bookmark(
        ctx: &CoreContext,
        coordinator: &mut BookmarksCoordinator,
        book: &BookmarkKey,
        expected_value: Option<(ChangesetId, BookmarkKind)>,
    ) -> Result<(), Error> {
        coordinator.update(ctx).await?;

        let coordinator = &coordinator;

        wait(|| async move {
            let val = coordinator
                .bookmarks
                .with_read(|bookmarks| bookmarks.get(book).cloned());
            Ok(val == expected_value)
        })
        .await
    }

    async fn wait<F, Fut>(func: F) -> Result<(), Error>
    where
        F: Fn() -> Fut,
        Fut: futures::future::Future<Output = Result<bool, Error>>,
    {
        let timeout_ms = 4000;
        time::timeout(Duration::from_millis(timeout_ms), async {
            loop {
                if func().await? {
                    break;
                }
                let sleep_ms = 10;
                time::sleep(Duration::from_millis(sleep_ms)).await;
            }

            Ok(())
        })
        .await?
    }

    #[mononoke::fbinit_test]
    async fn test_spawn_bookmarks_coordinator_failing_warmer(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .bookmarks()
            .get_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_key(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let failing_cs_id = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("failed", "failed")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "failingbook")
            .set_to(failing_cs_id)
            .await?;

        let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "master").set_to(master).await?;

        let warmer = Warmer {
            warmer: Box::new({
                cloned!(repo, failing_cs_id);
                move |ctx, cs_id| {
                    if cs_id == failing_cs_id {
                        async { Err(anyhow!("failed")) }.boxed()
                    } else {
                        cloned!(repo);
                        async move {
                            repo.repo_derived_data()
                                .derive::<RootUnodeManifestId>(ctx, cs_id)
                                .await?;
                            Ok(())
                        }
                        .boxed()
                    }
                }
            }),
            is_warm: Box::new({
                cloned!(repo);
                move |ctx, cs_id| {
                    cloned!(repo);
                    async move {
                        let res = repo
                            .repo_derived_data()
                            .fetch_derived::<RootUnodeManifestId>(ctx, cs_id)
                            .await?;
                        Ok(res.is_some())
                    }
                    .boxed()
                }
            }),
            name: "test".to_string(),
        };
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(warmer);
        let warmers = Arc::new(warmers);

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            repo.bookmarks_arc(),
            repo.bookmark_update_log_arc(),
            repo.repo_identity_arc(),
            repo.repo_event_publisher_arc(),
            warmers,
        );

        let master_book = BookmarkKey::new("master")?;
        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &master_book,
            Some((master, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        // This needs to split a bit because we don't know how long it would take for failing_book
        // to actually show up. :/
        tokio::time::sleep(Duration::from_secs(5)).await;

        let failing_book = BookmarkKey::new("failingbook")?;
        bookmarks.with_read(|bookmarks| assert_eq!(bookmarks.get(&failing_book), None));

        // Now change the warmer and make sure it derives successfully
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            repo.bookmarks_arc(),
            repo.bookmark_update_log_arc(),
            repo.repo_identity_arc(),
            repo.repo_event_publisher_arc(),
            warmers,
        );

        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &failing_book,
            Some((failing_cs_id, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_spawn_bookmarks_coordinator_check_single_updater(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        repo.repo_derived_data()
            .derive::<RootUnodeManifestId>(
                &ctx,
                repo.bookmarks()
                    .get(
                        ctx.clone(),
                        &BookmarkKey::new("master")?,
                        bookmarks::Freshness::MostRecent,
                    )
                    .await?
                    .unwrap(),
            )
            .await?;

        let derive_sleep_time_ms = 100;
        let how_many_derived = Arc::new(RwLock::new(HashMap::new()));
        let warmer = Warmer {
            warmer: Box::new({
                cloned!(repo, how_many_derived);
                move |ctx, cs_id| {
                    how_many_derived.with_write(|map| {
                        *map.entry(cs_id).or_insert(0) += 1;
                    });
                    cloned!(repo);
                    async move {
                        tokio::time::sleep(Duration::from_millis(derive_sleep_time_ms)).await;
                        repo.repo_derived_data()
                            .derive::<RootUnodeManifestId>(ctx, cs_id)
                            .await?;
                        Ok(())
                    }
                    .boxed()
                }
            }),
            is_warm: Box::new({
                cloned!(repo);
                move |ctx, cs_id| {
                    cloned!(repo);
                    async move {
                        let res = repo
                            .repo_derived_data()
                            .fetch_derived::<RootUnodeManifestId>(ctx, cs_id)
                            .await?
                            .is_some();
                        Ok(res)
                    }
                    .boxed()
                }
            }),
            name: "test".to_string(),
        };
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(warmer);
        let warmers = Arc::new(warmers);

        let bookmarks = repo
            .bookmarks()
            .get_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_key(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "master").set_to(master).await?;

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            repo.bookmarks_arc(),
            repo.bookmark_update_log_arc(),
            repo.repo_identity_arc(),
            repo.repo_event_publisher_arc(),
            warmers.clone(),
        );
        coordinator.update(&ctx).await?;

        // Give it a chance to derive
        wait({
            move || {
                cloned!(ctx, master, warmers);
                async move {
                    let res: Result<_, Error> = Ok(is_warm(&ctx, master, &warmers).await);
                    res
                }
            }
        })
        .await?;

        how_many_derived.with_read(|derived| {
            assert_eq!(derived.get(&master), Some(&1));
        });

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_spawn_bookmarks_coordinator_with_publishing_bookmarks(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: Repo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .bookmarks()
            .get_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_key(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;

        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        let new_cs_id = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "publishing")
            .create_publishing(new_cs_id)
            .await?;

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            repo.bookmarks_arc(),
            repo.bookmark_update_log_arc(),
            repo.repo_identity_arc(),
            repo.repo_event_publisher_arc(),
            warmers,
        );

        let publishing_book = BookmarkKey::new("publishing")?;
        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &publishing_book,
            Some((new_cs_id, BookmarkKind::Publishing)),
        )
        .await?;

        // Now recreate a bookmark with the same name but different kind
        bookmark(&ctx, &repo, "publishing").delete().await?;
        bookmark(&ctx, &repo, "publishing")
            .set_to(new_cs_id)
            .await?;

        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &publishing_book,
            Some((new_cs_id, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        Ok(())
    }

    mononoke_queries! {
        write ClearBookmarkUpdateLog(repo_id: RepositoryId) {
            none,
            "DELETE FROM bookmarks_update_log WHERE repo_id = {repo_id}"
        }
    }

    #[mononoke::fbinit_test]
    async fn test_single_bookmarks_no_history(fb: FacebookInit) -> Result<(), Error> {
        let factory = TestRepoFactory::new(fb)?;
        let repo: Repo = factory.build().await?;
        Linear::init_repo(fb, &repo).await?;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = Arc::new(RwLock::new(HashMap::new()));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId>(
            &ctx,
            repo.repo_derived_data_arc(),
        ));
        let warmers = Arc::new(warmers);

        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        warm_all(&ctx, master_cs_id, &warmers).await?;

        let master_book_name = BookmarkKey::new("master")?;
        let master_book = Bookmark::new(
            master_book_name.clone(),
            BookmarkKind::PullDefaultPublishing,
        );

        ClearBookmarkUpdateLog::query(
            &factory.metadata_db().write_connection,
            ctx.sql_query_telemetry(),
            &repo.repo_identity().id(),
        )
        .await?;

        single_bookmark_updater(&ctx, &repo, &master_book, &bookmarks, &warmers, |_| {}).await?;

        assert_eq!(
            bookmarks.with_read(|bookmarks| bookmarks.get(&master_book_name).cloned()),
            Some((master_cs_id, BookmarkKind::PullDefaultPublishing))
        );

        Ok(())
    }
}
