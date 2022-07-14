/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::ops::RangeBounds;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context as _;
use anyhow::Error;
use async_trait::async_trait;
use blame::BlameRoot;
use blame::RootBlameV2;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Bookmarks;
use bookmarks::BookmarksArc;
use bookmarks::BookmarksRef;
use bookmarks::BookmarksSubscription;
use bookmarks::Freshness;
use bookmarks_types::Bookmark;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkPagination;
use bookmarks_types::BookmarkPrefix;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use context::SessionClass;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use fastlog::RootFastlog;
use fsnodes::RootFsnodeId;
use futures::channel::oneshot;
use futures::future;
use futures::future::select;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use futures_watchdog::WatchdogExt;
use itertools::Itertools;
use lock_ext::RwLockExt;
use mercurial_derived_data::MappedHgChangesetId;
use metaconfig_types::BlameVersion;
use mononoke_types::ChangesetId;
use mononoke_types::Timestamp;
use phases::PhasesArc;
use repo_derived_data::RepoDerivedDataArc;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityArc;
use repo_identity::RepoIdentityRef;
use skeleton_manifest::RootSkeletonManifestId;
use slog::debug;
use slog::info;
use slog::warn;
use stats::prelude::*;
use tunables::tunables;
use unodes::RootUnodeManifestId;

mod warmers;
pub use warmers::create_derived_data_warmer;
pub use warmers::create_public_phase_warmer;

define_stats! {
    prefix = "mononoke.warm_bookmarks_cache";
    bookmark_discover_failures: timeseries(Rate, Sum),
    bookmark_update_failures: timeseries(Rate, Sum),
    max_staleness_secs: dynamic_singleton_counter("{}.max_staleness_secs", (reponame: String)),
}

pub struct WarmBookmarksCache {
    bookmarks: Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
    terminate: Option<oneshot::Sender<()>>,
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

pub struct WarmBookmarksCacheBuilder<'a, Repo> {
    ctx: CoreContext,
    repo: &'a Repo,
    warmers: Vec<Warmer>,
    init_mode: InitMode,
}

impl<'a, Repo> WarmBookmarksCacheBuilder<'a, Repo>
where
    Repo: BookmarksArc + BookmarkUpdateLogArc + RepoIdentityArc,
{
    pub fn new(mut ctx: CoreContext, repo: &'a Repo) -> Self {
        ctx.session_mut()
            .override_session_class(SessionClass::WarmBookmarksCache);
        let ctx = ctx.with_mutated_scuba(|mut scuba_sample_builder| {
            scuba_sample_builder.add("repo", repo.repo_identity().name().clone());
            scuba_sample_builder.add_common_server_data();
            scuba_sample_builder
        });

        Self {
            ctx,
            repo,
            warmers: vec![],
            init_mode: InitMode::Rewind,
        }
    }

    pub fn add_all_warmers(&mut self) -> Result<(), Error>
    where
        Repo: RepoDerivedDataArc + PhasesArc,
    {
        self.add_derived_data_warmers(&self.repo.repo_derived_data().active_config().types)?;
        self.add_public_phase_warmer();
        Ok(())
    }

    pub fn add_hg_warmers(&mut self) -> Result<(), Error>
    where
        Repo: RepoDerivedDataArc + PhasesArc,
    {
        self.add_derived_data_warmers(vec![MappedHgChangesetId::NAME, FilenodesOnlyPublic::NAME])?;
        self.add_public_phase_warmer();
        Ok(())
    }

    fn add_derived_data_warmers<'name, Name>(
        &mut self,
        types: impl IntoIterator<Item = &'name Name>,
    ) -> Result<(), Error>
    where
        Name: 'name + AsRef<str> + ?Sized,
        Repo: RepoDerivedDataArc,
    {
        let types = types.into_iter().map(AsRef::as_ref).collect::<HashSet<_>>();

        let config = self.repo.repo_derived_data().config();
        for ty in types.iter() {
            if !config.is_enabled(ty) {
                return Err(anyhow!(
                    "{} is not enabled for {}",
                    ty,
                    self.repo.repo_identity().name()
                ));
            }
        }

        if types.contains(MappedHgChangesetId::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<MappedHgChangesetId, _>(
                    &self.ctx, self.repo,
                ));
        }

        if types.contains(RootUnodeManifestId::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<RootUnodeManifestId, _>(
                    &self.ctx, self.repo,
                ));
        }
        if types.contains(RootFsnodeId::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<RootFsnodeId, _>(
                    &self.ctx, self.repo,
                ));
        }
        if types.contains(RootSkeletonManifestId::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<RootSkeletonManifestId, _>(
                    &self.ctx, self.repo,
                ));
        }
        if types.contains(BlameRoot::NAME) {
            match self.repo.repo_derived_data().active_config().blame_version {
                BlameVersion::V1 => {
                    self.warmers
                        .push(create_derived_data_warmer::<BlameRoot, _>(
                            &self.ctx, self.repo,
                        ));
                }
                BlameVersion::V2 => {
                    self.warmers
                        .push(create_derived_data_warmer::<RootBlameV2, _>(
                            &self.ctx, self.repo,
                        ));
                }
            }
        }
        if types.contains(ChangesetInfo::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<ChangesetInfo, _>(
                    &self.ctx, self.repo,
                ));
        }
        // deleted manifest share the same name
        if types.contains(RootDeletedManifestV2Id::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<RootDeletedManifestV2Id, _>(
                    &self.ctx, self.repo,
                ));
        }
        if types.contains(RootFastlog::NAME) {
            self.warmers
                .push(create_derived_data_warmer::<RootFastlog, _>(
                    &self.ctx, self.repo,
                ));
        }

        Ok(())
    }

    fn add_public_phase_warmer(&mut self)
    where
        Repo: PhasesArc,
    {
        let warmer = create_public_phase_warmer(&self.ctx, self.repo);
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
        WarmBookmarksCache::new(&self.ctx, self.repo, self.warmers, self.init_mode).await
    }
}

#[async_trait]
pub trait BookmarksCache: Send + Sync {
    async fn get(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
    ) -> Result<Option<ChangesetId>, Error>;

    async fn list(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkName, (ChangesetId, BookmarkKind))>, Error>;
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
        bookmark: &BookmarkName,
    ) -> Result<Option<ChangesetId>, Error> {
        self.bookmarks.get(ctx.clone(), bookmark).await
    }

    async fn list(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkName, (ChangesetId, BookmarkKind))>, Error> {
        self.bookmarks
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                prefix,
                BookmarkKind::ALL_PUBLISHING,
                pagination,
                limit.unwrap_or(std::u64::MAX),
            )
            .map_ok(|(book, cs_id)| (book.name, (cs_id, book.kind)))
            .try_collect()
            .await
    }
}

impl WarmBookmarksCache {
    pub async fn new(
        ctx: &CoreContext,
        repo: &(impl BookmarksArc + BookmarkUpdateLogArc + RepoIdentityArc),
        warmers: Vec<Warmer>,
        init_mode: InitMode,
    ) -> Result<Self, Error> {
        let warmers = Arc::new(warmers);
        let (sender, receiver) = oneshot::channel();

        info!(ctx.logger(), "Starting warm bookmark cache updater");
        let sub = repo
            .bookmarks()
            .create_subscription(ctx, Freshness::MaybeStale)
            .await
            .context("Error creating bookmarks subscription")?;

        let bookmarks = init_bookmarks(ctx, &*sub, repo, &warmers, init_mode).await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        BookmarksCoordinator::new(bookmarks.clone(), sub, repo, warmers.clone())
            .spawn(ctx.clone(), receiver);

        Ok(Self {
            bookmarks,
            terminate: Some(sender),
        })
    }
}

#[async_trait]
impl BookmarksCache for WarmBookmarksCache {
    async fn get(
        &self,
        _ctx: &CoreContext,
        bookmark: &BookmarkName,
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
    ) -> Result<Vec<(BookmarkName, (ChangesetId, BookmarkKind))>, Error> {
        let bookmarks = self.bookmarks.read().unwrap();

        if prefix.is_empty() && *pagination == BookmarkPagination::FromStart && limit.is_none() {
            // Simple case: return all bookmarks
            Ok(bookmarks
                .iter()
                .map(|(name, (cs_id, kind))| (name.clone(), (*cs_id, *kind)))
                .collect())
        } else {
            // Filter based on prefix and pagination
            let range = prefix.to_range().with_pagination(pagination.clone());
            let mut matches = bookmarks
                .iter()
                .filter(|(name, _)| range.contains(name))
                .map(|(name, (cs_id, kind))| (name.clone(), (*cs_id, *kind)))
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
    repo: &(impl BookmarksRef + BookmarkUpdateLogRef),
    warmers: &Arc<Vec<Warmer>>,
    mode: InitMode,
) -> Result<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>, Error> {
    let all_bookmarks = sub.bookmarks();
    let total = all_bookmarks.len();

    info!(ctx.logger(), "{} bookmarks to warm up", total);

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
                        let maybe_cs_id =
                            move_bookmark_back_in_history_until_derived(ctx, repo, &book, warmers)
                                .watched(ctx.logger())
                                .await?;

                        info!(
                            ctx.logger(),
                            "moved {} back in history to {:?}", book, maybe_cs_id
                        );
                        Ok((remaining, maybe_cs_id.map(|cs_id| (book, (cs_id, kind)))))
                    }
                    InitMode::Warm => {
                        info!(ctx.logger(), "warmed bookmark {} at {}", book, cs_id);
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
            if remaining % 100 == 0 {
                info!(ctx.logger(), "{} bookmarks left to warm up", remaining);
            }
            Result::<_, Error>::Ok(entry)
        })
        .try_collect::<HashMap<_, _>>()
        .await
        .with_context(|| "Error warming up bookmarks")?;

    info!(ctx.logger(), "all bookmarks are warmed up");

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
    repo: &(impl BookmarksRef + BookmarkUpdateLogRef),
    book: &BookmarkName,
    warmers: &Arc<Vec<Warmer>>,
) -> Result<Option<ChangesetId>, Error> {
    info!(ctx.logger(), "moving {} bookmark back in history...", book);

    let (latest_derived_entry, _) =
        find_all_underived_and_latest_derived(ctx, repo, book, warmers).await?;

    match latest_derived_entry {
        LatestDerivedBookmarkEntry::Found(maybe_cs_id_and_ts) => {
            Ok(maybe_cs_id_and_ts.map(|(cs_id, _)| cs_id))
        }
        LatestDerivedBookmarkEntry::NotFound => {
            let cur_bookmark_value = repo.bookmarks().get(ctx.clone(), book).await?;
            warn!(
                ctx.logger(),
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

pub struct BookmarkUpdateLogId(pub u64);

/// Searches bookmark log for latest entry for which everything is derived. Note that we consider log entry that
/// deletes a bookmark to be derived. Returns this entry if it was found and changesets for all underived entries after that
/// OLDEST ENTRIES FIRST.
pub async fn find_all_underived_and_latest_derived(
    ctx: &CoreContext,
    repo: &(impl BookmarksRef + BookmarkUpdateLogRef),
    book: &BookmarkName,
    warmers: &[Warmer],
) -> Result<
    (
        LatestDerivedBookmarkEntry,
        VecDeque<(ChangesetId, Option<(BookmarkUpdateLogId, Timestamp)>)>,
    ),
    Error,
> {
    let mut res = VecDeque::new();
    let history_depth_limits = vec![0, 10, 50, 100, 1000, 10000];

    for (prev_limit, limit) in history_depth_limits.into_iter().tuple_windows() {
        debug!(ctx.logger(), "{} bookmark, limit {}", book, limit);
        // Note that since new entries might be inserted to the bookmark log,
        // the next call to `list_bookmark_log_entries(...)` might return
        // entries that were already returned on the previous call `list_bookmark_log_entries(...)`.
        // That means that the same entry might be rechecked, but that shouldn't be a big problem.
        let mut log_entries = repo
            .bookmark_update_log()
            .list_bookmark_log_entries(
                ctx.clone(),
                book.clone(),
                limit,
                Some(prev_limit),
                Freshness::MaybeStale,
            )
            .map_ok(|(id, maybe_cs_id, _, ts)| {
                let id = BookmarkUpdateLogId(id);
                (maybe_cs_id, Some((id, ts)))
            })
            .try_collect::<Vec<_>>()
            .await?;

        if log_entries.is_empty() {
            debug!(ctx.logger(), "bookmark {} has no history in the log", book);
            let maybe_cs_id = repo.bookmarks().get(ctx.clone(), book).await?;
            // If a bookmark has no history then we add a fake entry saying that
            // timestamp is unknown.
            log_entries.push((maybe_cs_id, None));
        }

        let log_entries_fetched = log_entries.len();
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
                return Ok((LatestDerivedBookmarkEntry::Found(maybe_cs_ts), res));
            } else if let Some(cs_id_ts) = maybe_cs_id_ts {
                res.push_front(cs_id_ts);
            }
        }

        // Bookmark has been created recently and wasn't derived at all
        if (log_entries_fetched as u32) < limit {
            return Ok((LatestDerivedBookmarkEntry::Found(None), res));
        }
    }

    Ok((LatestDerivedBookmarkEntry::NotFound, res))
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
}

struct BookmarksCoordinator {
    bookmarks: Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
    sub: Box<dyn BookmarksSubscription>,
    repo: BookmarksCoordinatorRepo,
    warmers: Arc<Vec<Warmer>>,
    live_updaters: Arc<RwLock<HashMap<BookmarkName, BookmarkUpdaterState>>>,
}

impl BookmarksCoordinator {
    fn new(
        bookmarks: Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
        sub: Box<dyn BookmarksSubscription>,
        repo: &(impl BookmarksArc + BookmarkUpdateLogArc + RepoIdentityArc),
        warmers: Arc<Vec<Warmer>>,
    ) -> Self {
        let repo = BookmarksCoordinatorRepo {
            bookmarks: repo.bookmarks_arc(),
            bookmark_update_log: repo.bookmark_update_log_arc(),
            repo_identity: repo.repo_identity_arc(),
        };
        Self {
            bookmarks,
            sub,
            repo,
            warmers,
            live_updaters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn update(&mut self, ctx: &CoreContext) -> Result<(), Error> {
        // Report delay and remove finished updaters
        report_delay_and_remove_finished_updaters(
            ctx,
            &self.live_updaters,
            self.repo.repo_identity().name(),
        );

        let cur_bookmarks = self.bookmarks.with_read(|bookmarks| bookmarks.clone());

        let new_bookmarks = if tunables().get_warm_bookmark_cache_disable_subscription() {
            let books = self
                .repo
                .bookmarks()
                .list(
                    ctx.clone(),
                    Freshness::MaybeStale,
                    &BookmarkPrefix::empty(),
                    BookmarkKind::ALL_PUBLISHING,
                    &BookmarkPagination::FromStart,
                    std::u64::MAX,
                )
                .map_ok(|(book, cs_id)| {
                    let kind = *book.kind();
                    (book.into_name(), (cs_id, kind))
                })
                .try_collect::<HashMap<_, _>>()
                .await
                .context("Error fetching bookmarks")?;

            Cow::Owned(books)
        } else {
            self.sub
                .refresh(ctx)
                .await
                .context("Error refreshing subscription")?;

            Cow::Borrowed(self.sub.bookmarks())
        };

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
                if !live_updaters.contains_key(book.name()) {
                    live_updaters.insert(book.name().clone(), BookmarkUpdaterState::Started);
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
                let _ = tokio::spawn(async move {
                    let res = single_bookmark_updater(
                        &ctx,
                        &repo,
                        &book,
                        &bookmarks,
                        &warmers,
                        |ts: Timestamp| {
                            live_updaters.with_write(|live_updaters| {
                                live_updaters.insert(
                                    book.name().clone(),
                                    BookmarkUpdaterState::InProgress {
                                        oldest_underived_ts: ts,
                                    },
                                );
                            });
                        },
                    )
                    .await;
                    if let Err(ref err) = res {
                        STATS::bookmark_update_failures.add_value(1);
                        warn!(ctx.logger(), "update of {} failed: {:?}", book.name(), err);
                    };

                    live_updaters.with_write(|live_updaters| {
                        let maybe_state = live_updaters.remove(book.name());
                        if let Some(state) = maybe_state {
                            live_updaters.insert(book.name().clone(), state.into_finished(&res));
                        }
                    });
                });
            }
        }

        Ok(())
    }

    // Loop that finds bookmarks that were modified and spawns separate bookmark updaters for them
    pub fn spawn(mut self, ctx: CoreContext, terminate: oneshot::Receiver<()>) {
        let fut = async move {
            info!(ctx.logger(), "Started warm bookmark cache updater");
            let infinite_loop = async {
                loop {
                    let res = self.update(&ctx).await;

                    if let Err(err) = res.as_ref() {
                        STATS::bookmark_discover_failures.add_value(1);
                        warn!(ctx.logger(), "failed to update bookmarks {:?}", err);
                    }

                    let delay_ms = match tunables()
                        .get_warm_bookmark_cache_poll_interval_ms()
                        .try_into()
                    {
                        Ok(duration) if duration > 0 => duration,
                        _ => 1000,
                    };

                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
            .boxed();

            let _ = select(infinite_loop, terminate).await;

            info!(ctx.logger(), "Stopped warm bookmark cache updater");
        };

        // Detach the handle. This will terminate using the `terminate` receiver.
        let _ = tokio::task::spawn(fut);
    }
}

fn report_delay_and_remove_finished_updaters(
    ctx: &CoreContext,
    live_updaters: &Arc<RwLock<HashMap<BookmarkName, BookmarkUpdaterState>>>,
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

    STATS::max_staleness_secs.set_value(ctx.fb, max_staleness as i64, (reponame.to_owned(),));
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
    bookmarks: &Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
    warmers: &Arc<Vec<Warmer>>,
    mut staleness_reporter: impl FnMut(Timestamp),
) -> Result<(), Error> {
    let (latest_derived, underived_history) =
        find_all_underived_and_latest_derived(ctx, repo, bookmark.name(), warmers.as_ref()).await?;

    let update_bookmark = |cs_id: ChangesetId| async move {
        bookmarks.with_write(|bookmarks| {
            let name = bookmark.name().clone();
            bookmarks.insert(name, (cs_id, *bookmark.kind()))
        });
    };

    match latest_derived {
        // Move bookmark to the latest derived commit or delete the bookmark completely
        LatestDerivedBookmarkEntry::Found(maybe_cs_id_and_ts) => match maybe_cs_id_and_ts {
            Some((cs_id, _ts)) => {
                update_bookmark(cs_id).await;
            }
            None => {
                bookmarks.with_write(|bookmarks| bookmarks.remove(bookmark.name()));
            }
        },
        LatestDerivedBookmarkEntry::NotFound => {
            warn!(
                ctx.logger(),
                "Haven't found previous derived version of {}! Will try to derive anyway",
                bookmark.name()
            );
        }
    }

    for (underived_cs_id, maybe_id_ts) in underived_history {
        if let Some((_, ts)) = maybe_id_ts {
            // timestamp might not be known if e.g. bookmark has no history.
            // In that case let's not report staleness
            staleness_reporter(ts);
        }

        let bookmark_log_id = maybe_id_ts.as_ref().map(|(id, _)| id.0);
        let maybe_ts = maybe_id_ts.map(|(_, ts)| ts);

        let ctx = ctx.clone().with_mutated_scuba(|mut scuba| {
            scuba.add("bookmark", bookmark.name().to_string());
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
                warn!(
                    ctx.logger(),
                    "failed to derive data for {} while updating {}: {}",
                    underived_cs_id,
                    bookmark.name(),
                    err
                );
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use cloned::cloned;
    use delayblob::DelayedBlobstore;
    use derived_data::BonsaiDerived;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use maplit::hashmap;
    use memblob::Memblob;
    use mononoke_api_types::InnerRepo;
    use mononoke_types::RepositoryId;
    use sql::queries;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::bookmark;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;
    use tokio::time;

    #[fbinit::test]
    async fn test_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let sub = repo
            .blob_repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        // Unodes haven't been derived at all - so we should get an empty set of bookmarks
        let bookmarks = init_bookmarks(&ctx, &*sub, &repo, &warmers, InitMode::Rewind).await?;
        assert_eq!(bookmarks, HashMap::new());

        let master_cs_id = resolve_cs_id(&ctx, &repo.blob_repo, "master").await?;
        RootUnodeManifestId::derive(&ctx, &repo.blob_repo, master_cs_id).await?;

        let bookmarks = init_bookmarks(&ctx, &*sub, &repo, &warmers, InitMode::Rewind).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (master_cs_id, BookmarkKind::PullDefaultPublishing)}
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_find_derived(fb: FacebookInit) -> Result<(), Error> {
        let put_distr = rand_distr::Normal::<f64>::new(0.1, 0.05).unwrap();
        let get_distr = rand_distr::Normal::<f64>::new(0.05, 0.025).unwrap();
        let blobstore = Arc::new(DelayedBlobstore::new(
            Memblob::default(),
            put_distr,
            get_distr,
        ));
        let repo: InnerRepo = TestRepoFactory::new(fb)?
            .with_blobstore(blobstore)
            .build()?;
        Linear::initrepo(fb, &repo.blob_repo).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        info!(ctx.logger(), "creating 5 derived commits");
        let mut master = resolve_cs_id(&ctx, &repo.blob_repo, "master").await?;
        for _ in 1..5 {
            let new_master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec![master])
                .commit()
                .await?;

            bookmark(&ctx, &repo.blob_repo, "master")
                .set_to(new_master)
                .await?;
            master = new_master;
        }
        RootUnodeManifestId::derive(&ctx, &repo.blob_repo, master).await?;
        let derived_master = master;

        info!(ctx.logger(), "creating 5 more underived commits");
        for _ in 1..5 {
            let new_master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec![master])
                .commit()
                .await?;
            bookmark(&ctx, &repo.blob_repo, "master")
                .set_to(new_master)
                .await?;
            master = new_master;
        }

        let sub = repo
            .blob_repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let bookmarks = init_bookmarks(&ctx, &*sub, &repo, &warmers, InitMode::Rewind).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        RootUnodeManifestId::derive(&ctx, &repo.blob_repo, master).await?;
        let bookmarks = init_bookmarks(&ctx, &*sub, &repo, &warmers, InitMode::Rewind).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_a_lot_of_moves(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        let derived_master = resolve_cs_id(&ctx, &repo.blob_repo, "master").await?;
        RootUnodeManifestId::derive(&ctx, &repo.blob_repo, derived_master).await?;

        for i in 1..50 {
            let new_master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
                .add_file(format!("{}", i).as_str(), "content")
                .commit()
                .await?;

            bookmark(&ctx, &repo.blob_repo, "master")
                .set_to(new_master)
                .await?;
        }

        let sub = repo
            .blob_repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let bookmarks = init_bookmarks(&ctx, &*sub, &repo, &warmers, InitMode::Rewind).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_derived_right_after_threshold(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        let derived_master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        RootUnodeManifestId::derive(&ctx, &repo.blob_repo, derived_master).await?;
        bookmark(&ctx, &repo.blob_repo, "master")
            .set_to(derived_master)
            .await?;

        // First history threshold is 10. Let's make sure we don't have off-by one errors
        for i in 0..10 {
            let new_master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
                .add_file(format!("{}", i).as_str(), "content")
                .commit()
                .await?;

            bookmark(&ctx, &repo.blob_repo, "master")
                .set_to(new_master)
                .await?;
        }

        let sub = repo
            .blob_repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await?;

        let bookmarks = init_bookmarks(&ctx, &*sub, &repo, &warmers, InitMode::Rewind).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_spawn_bookmarks_coordinator_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .blob_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        let master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo.blob_repo, "master")
            .set_to(master)
            .await?;

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.blob_repo
                .bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            &repo,
            warmers,
        );

        let master_book = BookmarkName::new("master")?;
        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &master_book,
            Some((master, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        bookmark(&ctx, &repo.blob_repo, "master").delete().await?;
        update_and_wait_for_bookmark(&ctx, &mut coordinator, &master_book, None).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_single_bookmarks_coordinator_many_updates(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .blob_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        info!(ctx.logger(), "created stack of commits");
        for i in 1..10 {
            let master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
                .add_file(format!("somefile{}", i).as_str(), "content")
                .commit()
                .await?;
            info!(ctx.logger(), "created {}", master);
            bookmark(&ctx, &repo.blob_repo, "master")
                .set_to(master)
                .await?;
        }
        let master_cs_id = resolve_cs_id(&ctx, &repo.blob_repo, "master").await?;
        info!(ctx.logger(), "created the whole stack of commits");

        let master_book_name = BookmarkName::new("master")?;
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
        book: &BookmarkName,
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
        let res = time::timeout(Duration::from_millis(timeout_ms), async {
            loop {
                if func().await? {
                    break;
                }
                let sleep_ms = 10;
                time::sleep(Duration::from_millis(sleep_ms)).await;
            }

            Ok(())
        })
        .await?;
        res
    }

    #[fbinit::test]
    async fn test_spawn_bookmarks_coordinator_failing_warmer(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .blob_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let failing_cs_id = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
            .add_file("failed", "failed")
            .commit()
            .await?;
        bookmark(&ctx, &repo.blob_repo, "failingbook")
            .set_to(failing_cs_id)
            .await?;

        let master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo.blob_repo, "master")
            .set_to(master)
            .await?;

        let warmer = Warmer {
            warmer: Box::new({
                cloned!(repo, failing_cs_id);
                move |ctx, cs_id| {
                    if cs_id == failing_cs_id {
                        async { Err(anyhow!("failed")) }.boxed()
                    } else {
                        cloned!(repo);
                        async move {
                            RootUnodeManifestId::derive(ctx, &repo.blob_repo, cs_id).await?;
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
                        let res =
                            RootUnodeManifestId::is_derived(ctx, &repo.blob_repo, &cs_id).await?;
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

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.blob_repo
                .bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            &repo,
            warmers,
        );

        let master_book = BookmarkName::new("master")?;
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

        let failing_book = BookmarkName::new("failingbook")?;
        bookmarks.with_read(|bookmarks| assert_eq!(bookmarks.get(&failing_book), None));

        // Now change the warmer and make sure it derives successfully
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.blob_repo
                .bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            &repo,
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

    #[fbinit::test]
    async fn test_spawn_bookmarks_coordinator_check_single_updater(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        RootUnodeManifestId::derive(
            &ctx,
            &repo.blob_repo,
            repo.blob_repo
                .get_bonsai_bookmark(ctx.clone(), &BookmarkName::new("master")?)
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
                        RootUnodeManifestId::derive(ctx, &repo.blob_repo, cs_id).await?;
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
                        let res =
                            RootUnodeManifestId::is_derived(ctx, &repo.blob_repo, &cs_id).await?;
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
            .blob_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let master = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo.blob_repo, "master")
            .set_to(master)
            .await?;

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.blob_repo
                .bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            &repo,
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

    #[fbinit::test]
    async fn test_spawn_bookmarks_coordinator_with_publishing_bookmarks(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = Linear::get_inner_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .blob_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;

        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        let new_cs_id = CreateCommitContext::new(&ctx, &repo.blob_repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo.blob_repo, "publishing")
            .create_publishing(new_cs_id)
            .await?;

        let mut coordinator = BookmarksCoordinator::new(
            bookmarks.clone(),
            repo.blob_repo
                .bookmarks()
                .create_subscription(&ctx, Freshness::MostRecent)
                .await?,
            &repo,
            warmers,
        );

        let publishing_book = BookmarkName::new("publishing")?;
        update_and_wait_for_bookmark(
            &ctx,
            &mut coordinator,
            &publishing_book,
            Some((new_cs_id, BookmarkKind::Publishing)),
        )
        .await?;

        // Now recreate a bookmark with the same name but different kind
        bookmark(&ctx, &repo.blob_repo, "publishing")
            .delete()
            .await?;
        bookmark(&ctx, &repo.blob_repo, "publishing")
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

    queries! {
        write ClearBookmarkUpdateLog(repo_id: RepositoryId) {
            none,
            "DELETE FROM bookmarks_update_log WHERE repo_id = {repo_id}"
        }
    }

    #[fbinit::test]
    async fn test_single_bookmarks_no_history(fb: FacebookInit) -> Result<(), Error> {
        let factory = TestRepoFactory::new(fb)?;
        let repo: InnerRepo = factory.build()?;
        Linear::initrepo(fb, &repo.blob_repo).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = Arc::new(RwLock::new(HashMap::new()));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_derived_data_warmer::<RootUnodeManifestId, _>(
            &ctx, &repo,
        ));
        let warmers = Arc::new(warmers);

        let master_cs_id = resolve_cs_id(&ctx, &repo.blob_repo, "master").await?;
        warm_all(&ctx, master_cs_id, &warmers).await?;

        let master_book_name = BookmarkName::new("master")?;
        let master_book = Bookmark::new(
            master_book_name.clone(),
            BookmarkKind::PullDefaultPublishing,
        );

        ClearBookmarkUpdateLog::query(
            &factory.metadata_db().connections().write_connection,
            &repo.blob_repo.get_repoid(),
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
