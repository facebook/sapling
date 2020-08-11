/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Error};
use blame::BlameRoot;
use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, Freshness};
use bookmarks_types::{Bookmark, BookmarkKind};
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::{
    channel::oneshot,
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, select, BoxFuture, FutureExt as NewFutureExt, TryFutureExt},
    stream::{self, FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_ext::{BoxFuture as OldBoxFuture, FutureExt};
use futures_old::Future;
use itertools::Itertools;
use lock_ext::RwLockExt;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_types::{ChangesetId, Timestamp};
use slog::{debug, info, warn};
use stats::prelude::*;
use tunables::tunables;
use unodes::RootUnodeManifestId;

define_stats! {
    prefix = "mononoke.warm_bookmarks_cache";
    bookmarks_fetch_failures: timeseries(Rate, Sum),
    bookmark_update_failures: timeseries(Rate, Sum),
    max_staleness_secs: dynamic_singleton_counter("{}.max_staleness_secs", (reponame: String)),
}

pub struct WarmBookmarksCache {
    bookmarks: Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
    terminate: Option<oneshot::Sender<()>>,
}

pub type WarmerFn =
    dyn Fn(CoreContext, BlobRepo, ChangesetId) -> OldBoxFuture<(), Error> + Send + Sync + 'static;

pub type IsWarmFn = dyn for<'a> Fn(&'a CoreContext, &'a BlobRepo, &'a ChangesetId) -> BoxFuture<'a, Result<bool, Error>>
    + Send
    + Sync;

#[derive(Clone, Copy)]
pub enum BookmarkUpdateDelay {
    Allow,
    Disallow,
}

pub struct Warmer {
    warmer: Box<WarmerFn>,
    is_warm: Box<IsWarmFn>,
}

pub fn create_warmer<D: BonsaiDerived>(ctx: &CoreContext) -> Warmer {
    info!(ctx.logger(), "Warming {}", D::NAME);
    let warmer: Box<WarmerFn> = Box::new(|ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| {
        D::derive(ctx, repo, cs_id)
            .map(|_| ())
            .map_err(Error::from)
            .boxify()
    });

    let is_warm: Box<IsWarmFn> =
        Box::new(|ctx: &CoreContext, repo: &BlobRepo, cs_id: &ChangesetId| {
            D::is_derived(&ctx, &repo, &cs_id)
                .map_err(Error::from)
                .boxed()
        });
    Warmer { warmer, is_warm }
}

pub struct WarmBookmarksCacheBuilder<'a> {
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    warmers: Vec<Warmer>,
}

impl<'a> WarmBookmarksCacheBuilder<'a> {
    pub fn new(ctx: &'a CoreContext, repo: &'a BlobRepo) -> Self {
        Self {
            ctx,
            repo,
            warmers: vec![],
        }
    }

    pub fn add_all_derived_data_warmers(&mut self) -> Result<(), Error> {
        self.add_derived_data_warmers(&self.repo.get_derived_data_config().derived_data_types)
    }

    pub fn add_derived_data_warmers(&mut self, types: &BTreeSet<String>) -> Result<(), Error> {
        let derived_data_types = &self.repo.get_derived_data_config().derived_data_types;
        for ty in types {
            if !derived_data_types.contains(ty) {
                return Err(anyhow!("{} is not enabled for {}", ty, self.repo.name()));
            }
        }

        if types.contains(MappedHgChangesetId::NAME) {
            self.warmers
                .push(create_warmer::<MappedHgChangesetId>(&self.ctx));
        }

        if types.contains(RootUnodeManifestId::NAME) {
            self.warmers
                .push(create_warmer::<RootUnodeManifestId>(&self.ctx));
        }
        if types.contains(RootFsnodeId::NAME) {
            self.warmers.push(create_warmer::<RootFsnodeId>(&self.ctx));
        }
        if types.contains(BlameRoot::NAME) {
            self.warmers.push(create_warmer::<BlameRoot>(&self.ctx));
        }
        if types.contains(ChangesetInfo::NAME) {
            self.warmers.push(create_warmer::<ChangesetInfo>(&self.ctx));
        }
        if types.contains(RootDeletedManifestId::NAME) {
            self.warmers
                .push(create_warmer::<RootDeletedManifestId>(&self.ctx));
        }

        Ok(())
    }

    pub async fn build(
        self,
        bookmark_update_delay: BookmarkUpdateDelay,
    ) -> Result<WarmBookmarksCache, Error> {
        WarmBookmarksCache::new(&self.ctx, &self.repo, bookmark_update_delay, self.warmers).await
    }
}

impl WarmBookmarksCache {
    pub async fn new(
        ctx: &CoreContext,
        repo: &BlobRepo,
        bookmark_update_delay: BookmarkUpdateDelay,
        warmers: Vec<Warmer>,
    ) -> Result<Self, Error> {
        let warmers = Arc::new(warmers);
        let (sender, receiver) = oneshot::channel();

        info!(ctx.logger(), "Starting warm bookmark cache updater");
        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let loop_sleep = Duration::from_millis(1000);
        spawn_bookmarks_coordinator(
            bookmarks.clone(),
            receiver,
            ctx.clone(),
            repo.clone(),
            warmers.clone(),
            loop_sleep,
            bookmark_update_delay,
        );
        Ok(Self {
            bookmarks,
            terminate: Some(sender),
        })
    }

    pub fn get(&self, bookmark: &BookmarkName) -> Option<ChangesetId> {
        self.bookmarks
            .read()
            .unwrap()
            .get(bookmark)
            .map(|(cs_id, _)| cs_id)
            .cloned()
    }

    pub fn get_all(&self) -> HashMap<BookmarkName, (ChangesetId, BookmarkKind)> {
        self.bookmarks.read().unwrap().clone()
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
    repo: &BlobRepo,
    warmers: &Arc<Vec<Warmer>>,
) -> Result<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>, Error> {
    let all_bookmarks = repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .compat()
        .try_collect::<HashMap<_, _>>()
        .await?;

    all_bookmarks
        .into_iter()
        .map(|(book, cs_id)| async move {
            let kind = *book.kind();
            if !is_warm(ctx, repo, &cs_id, warmers).await {
                let book_name = book.into_name();
                let maybe_cs_id =
                    move_bookmark_back_in_history_until_derived(&ctx, &repo, &book_name, &warmers)
                        .await?;

                info!(
                    ctx.logger(),
                    "moved {} back in history to {:?}", book_name, maybe_cs_id
                );
                Ok(maybe_cs_id.map(|cs_id| (book_name, (cs_id, kind))))
            } else {
                Ok(Some((book.into_name(), (cs_id, kind))))
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_filter_map(|x| async { Ok(x) })
        .try_collect::<HashMap<_, _>>()
        .await
}

async fn is_warm(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: &ChangesetId,
    warmers: &[Warmer],
) -> bool {
    let is_warm = warmers
        .iter()
        .map(|warmer| (*warmer.is_warm)(ctx, repo, cs_id).map(|res| res.unwrap_or(false)))
        .collect::<FuturesUnordered<_>>();

    is_warm
        .fold(true, |acc, is_warm| future::ready(acc & is_warm))
        .await
}

async fn warm_all(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: &ChangesetId,
    warmers: &Arc<Vec<Warmer>>,
) -> Result<(), Error> {
    stream::iter(warmers.iter().map(Ok))
        .try_for_each_concurrent(100, |warmer| {
            (*warmer.warmer)(ctx.clone(), repo.clone(), *cs_id).compat()
        })
        .await
}

async fn move_bookmark_back_in_history_until_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
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
            let cur_bookmark_value = repo.get_bonsai_bookmark(ctx.clone(), book).compat().await?;
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
    Found(Option<(ChangesetId, Timestamp)>),
    /// Latest derived bookmark entry is too far away
    NotFound,
}

/// Searches bookmark log for latest entry for which everything is derived. Note that we consider log entry that
/// deletes a bookmark to be derived. Returns this entry if it was found and changesets for all underived entries after that
/// OLDEST ENTRIES FIRST.
pub async fn find_all_underived_and_latest_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    book: &BookmarkName,
    warmers: &[Warmer],
) -> Result<
    (
        LatestDerivedBookmarkEntry,
        VecDeque<(ChangesetId, Timestamp)>,
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
        let log_entries = repo
            .list_bookmark_log_entries(
                ctx.clone(),
                book.clone(),
                limit,
                Some(prev_limit),
                Freshness::MaybeStale,
            )
            .compat()
            .try_collect::<Vec<_>>()
            .await?;

        let log_entries_fetched = log_entries.len();
        let mut maybe_derived = stream::iter(log_entries.into_iter().map(
            |(maybe_cs_id, _, ts)| async move {
                match maybe_cs_id {
                    Some(cs_id) => {
                        let derived = is_warm(ctx, repo, &cs_id, warmers).await;
                        (Some((cs_id, ts)), derived)
                    }
                    None => (None, true),
                }
            },
        ))
        .buffered(100);

        while let Some((maybe_cs_and_ts, is_warm)) = maybe_derived.next().await {
            if is_warm {
                return Ok((LatestDerivedBookmarkEntry::Found(maybe_cs_and_ts), res));
            } else {
                if let Some(cs_and_ts) = maybe_cs_and_ts {
                    res.push_front(cs_and_ts);
                }
            }
        }

        // Bookmark has been created recently and wasn't derived at all
        if (log_entries_fetched as u32) < limit {
            return Ok((LatestDerivedBookmarkEntry::Found(None), res));
        }
    }

    Ok((LatestDerivedBookmarkEntry::NotFound, res))
}

// Loop that finds bookmarks that were modified and spawns separate bookmark updaters for them
fn spawn_bookmarks_coordinator(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
    terminate: oneshot::Receiver<()>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Warmer>>,
    loop_sleep: Duration,
    bookmark_update_delay: BookmarkUpdateDelay,
) {
    // ignore JoinHandle, because we want it to run until `terminate` receives a signal
    let _ = tokio::spawn(async move {
        info!(ctx.logger(), "Started warm bookmark cache updater");
        let infinite_loop = async {
            // This set is used to keep track of which bookmark is being updated
            // and make sure that we don't have more than a single updater for a bookmark.
            let live_updaters = Arc::new(RwLock::new(
                HashMap::<BookmarkName, BookmarkUpdaterState>::new(),
            ));
            loop {
                // Report delay and remove finished updaters
                report_delay_and_remove_finished_updaters(&ctx, &live_updaters, &repo.name());

                let cur_bookmarks = bookmarks.with_read(|bookmarks| bookmarks.clone());

                let res = repo
                    .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
                    .compat()
                    .map_ok(|(book, cs_id)| {
                        let kind = *book.kind();
                        (book.into_name(), (cs_id, kind))
                    })
                    .try_collect::<HashMap<_, _>>()
                    .await;

                let new_bookmarks = match res {
                    Ok(bookmarks) => bookmarks,
                    Err(err) => {
                        STATS::bookmarks_fetch_failures.add_value(1);
                        warn!(ctx.logger(), "failed to fetch bookmarks {:?}", err);
                        continue;
                    }
                };

                let mut changed_bookmarks = vec![];
                // Find bookmarks that were moved/created and spawn an updater
                // for them
                for (key, new_value) in &new_bookmarks {
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
                    let need_spawning = live_updaters.with_write(|live_updaters| {
                        if !live_updaters.contains_key(book.name()) {
                            live_updaters
                                .insert(book.name().clone(), BookmarkUpdaterState::Started);
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
                        cloned!(ctx, repo, bookmarks, live_updaters, warmers);
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
                                bookmark_update_delay,
                            )
                            .await;
                            if let Err(ref err) = res {
                                STATS::bookmark_update_failures.add_value(1);
                                warn!(ctx.logger(), "update of {} failed: {:?}", book.name(), err);
                            };

                            live_updaters.with_write(|live_updaters| {
                                let maybe_state = live_updaters.remove(&book.name());
                                if let Some(state) = maybe_state {
                                    live_updaters
                                        .insert(book.name().clone(), state.into_finished(&res));
                                }
                            });
                        });
                    }
                }

                tokio::time::delay_for(loop_sleep).await;
            }
        }
        .boxed();

        let _ = select(infinite_loop, terminate).await;

        info!(ctx.logger(), "Stopped warm bookmark cache updater");
        let res: Result<_, Error> = Ok(());
        res
    });
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
    repo: &BlobRepo,
    bookmark: &Bookmark,
    bookmarks: &Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
    warmers: &Arc<Vec<Warmer>>,
    mut staleness_reporter: impl FnMut(Timestamp),
    bookmark_update_delay: BookmarkUpdateDelay,
) -> Result<(), Error> {
    let (latest_derived, underived_history) =
        find_all_underived_and_latest_derived(&ctx, &repo, &bookmark.name(), warmers.as_ref())
            .await?;

    let bookmark_update_delay_secs = match bookmark_update_delay {
        BookmarkUpdateDelay::Allow => {
            let delay_secs = tunables().get_warm_bookmark_cache_delay();
            if delay_secs < 0 {
                warn!(
                    ctx.logger(),
                    "invalid warm bookmark cache delay value: {}", delay_secs
                );
            }
            delay_secs
        }
        BookmarkUpdateDelay::Disallow => 0,
    };

    let update_bookmark = |ts: Timestamp, cs_id: ChangesetId| async move {
        let cur_delay = ts.since_seconds();
        if cur_delay < bookmark_update_delay_secs {
            let to_sleep = (bookmark_update_delay_secs - cur_delay) as u64;
            info!(
                ctx.logger(),
                "sleeping for {} secs before updating a bookmark", to_sleep
            );
            tokio::time::delay_for(Duration::from_secs(to_sleep)).await;
        }
        bookmarks.with_write(|bookmarks| {
            let name = bookmark.name().clone();
            bookmarks.insert(name, (cs_id, *bookmark.kind()))
        });
    };

    match latest_derived {
        // Move bookmark to the latest derived commit or delete the bookmark completely
        LatestDerivedBookmarkEntry::Found(maybe_cs_id_and_ts) => match maybe_cs_id_and_ts {
            Some((cs_id, ts)) => {
                update_bookmark(ts, cs_id).await;
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

    for (underived_cs_id, ts) in underived_history {
        staleness_reporter(ts);

        let res = warm_all(&ctx, &repo, &underived_cs_id, &warmers).await;
        match res {
            Ok(()) => {
                update_bookmark(ts, underived_cs_id).await;
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
    use blobrepo_override::DangerousOverride;
    use blobstore::Blobstore;
    use cloned::cloned;
    use delayblob::DelayedBlobstore;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use maplit::hashmap;
    use tests_utils::{bookmark, resolve_cs_id, CreateCommitContext};
    use tokio::time;

    const TEST_LOOP_SLEEP: Duration = Duration::from_millis(1);

    #[fbinit::compat_test]
    async fn test_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        // Unodes haven't been derived at all - so we should get an empty set of bookmarks
        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(bookmarks, HashMap::new());

        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), master_cs_id)
            .compat()
            .await?;

        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (master_cs_id, BookmarkKind::PullDefaultPublishing)}
        );
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_find_derived(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let repo = repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            let put_distr = rand_distr::Normal::<f64>::new(0.1, 0.05).unwrap();
            let get_distr = rand_distr::Normal::<f64>::new(0.05, 0.025).unwrap();
            Arc::new(DelayedBlobstore::new(blobstore, put_distr, get_distr))
        });
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        info!(ctx.logger(), "creating 5 derived commits");
        let mut master = resolve_cs_id(&ctx, &repo, "master").await?;
        for _ in 1..5 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec![master])
                .commit()
                .await?;

            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
            master = new_master;
        }
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), master)
            .compat()
            .await?;
        let derived_master = master;

        info!(ctx.logger(), "creating 5 more underived commits");
        for _ in 1..5 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec![master])
                .commit()
                .await?;
            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
            master = new_master;
        }

        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), master)
            .compat()
            .await?;
        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_a_lot_of_moves(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        let derived_master = resolve_cs_id(&ctx, &repo, "master").await?;
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), derived_master)
            .compat()
            .await?;

        for i in 1..50 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
                .add_file(format!("{}", i), "content")
                .commit()
                .await?;

            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
        }

        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_derived_right_after_threshold(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        let derived_master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), derived_master)
            .compat()
            .await?;
        bookmark(&ctx, &repo, "master")
            .set_to(derived_master)
            .await?;

        // First history threshold is 10. Let's make sure we don't have off-by one errors
        for i in 0..10 {
            let new_master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
                .add_file(format!("{}", i), "content")
                .commit()
                .await?;

            bookmark(&ctx, &repo, "master").set_to(new_master).await?;
        }

        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(
            bookmarks,
            hashmap! {BookmarkName::new("master")? => (derived_master, BookmarkKind::PullDefaultPublishing)}
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_spawn_bookmarks_coordinator_simple(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .compat()
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "master").set_to(master).await?;

        let (cancel, receiver_cancel) = oneshot::channel();
        spawn_bookmarks_coordinator(
            bookmarks.clone(),
            receiver_cancel,
            ctx.clone(),
            repo.clone(),
            warmers,
            TEST_LOOP_SLEEP,
            BookmarkUpdateDelay::Disallow,
        );

        let master_book = BookmarkName::new("master")?;
        wait_for_bookmark(
            &bookmarks,
            &master_book,
            Some((master, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        bookmark(&ctx, &repo, "master").delete().await?;
        wait_for_bookmark(&bookmarks, &master_book, None).await?;

        let _ = cancel.send(());

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_single_bookmarks_coordinator_many_updates(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .compat()
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        info!(ctx.logger(), "created stack of commits");
        for i in 1..10 {
            let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
                .add_file(format!("somefile{}", i), "content")
                .commit()
                .await?;
            info!(ctx.logger(), "created {}", master);
            bookmark(&ctx, &repo, "master").set_to(master).await?;
        }
        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        info!(ctx.logger(), "created the whole stack of commits");

        let master_book_name = BookmarkName::new("master")?;
        let master_book = Bookmark::new(
            master_book_name.clone(),
            BookmarkKind::PullDefaultPublishing,
        );
        single_bookmark_updater(
            &ctx,
            &repo,
            &master_book,
            &bookmarks,
            &warmers,
            |_| {},
            BookmarkUpdateDelay::Disallow,
        )
        .await?;

        assert_eq!(
            bookmarks.with_read(|bookmarks| bookmarks.get(&master_book_name).cloned()),
            Some((master_cs_id, BookmarkKind::PullDefaultPublishing))
        );

        Ok(())
    }

    async fn wait_for_bookmark(
        bookmarks: &Arc<RwLock<HashMap<BookmarkName, (ChangesetId, BookmarkKind)>>>,
        book: &BookmarkName,
        expected_value: Option<(ChangesetId, BookmarkKind)>,
    ) -> Result<(), Error> {
        wait(|| async move {
            Ok(bookmarks.with_read(|bookmarks| bookmarks.get(book).cloned()) == expected_value)
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
                time::delay_for(Duration::from_millis(sleep_ms)).await;
            }

            Ok(())
        })
        .await?;
        res
    }

    #[fbinit::compat_test]
    async fn test_spawn_bookmarks_coordinator_failing_warmer(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .compat()
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
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
                cloned!(failing_cs_id);
                move |ctx, repo, cs_id| {
                    if cs_id == failing_cs_id {
                        futures_old::future::err(anyhow!("failed")).boxify()
                    } else {
                        RootUnodeManifestId::derive(ctx, repo, cs_id)
                            .map(|_| ())
                            .from_err()
                            .boxify()
                    }
                }
            }),
            is_warm: Box::new(|ctx, repo, cs_id| {
                async move {
                    let res = RootUnodeManifestId::is_derived(&ctx, &repo, &cs_id).await?;
                    Ok(res)
                }
                .boxed()
            }),
        };
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(warmer);
        let warmers = Arc::new(warmers);

        let (cancel, receiver_cancel) = oneshot::channel();
        spawn_bookmarks_coordinator(
            bookmarks.clone(),
            receiver_cancel,
            ctx.clone(),
            repo.clone(),
            warmers,
            TEST_LOOP_SLEEP,
            BookmarkUpdateDelay::Disallow,
        );

        let master_book = BookmarkName::new("master")?;
        wait_for_bookmark(
            &bookmarks,
            &master_book,
            Some((master, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;
        let _ = cancel.send(());

        let failing_book = BookmarkName::new("failingbook")?;
        bookmarks.with_read(|bookmarks| assert_eq!(bookmarks.get(&failing_book), None));

        // Give a chance to first coordinator to finish
        tokio::time::delay_for(TEST_LOOP_SLEEP * 5).await;

        // Now change the warmer and make sure it derives successfully
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        let (cancel, receiver_cancel) = oneshot::channel();
        spawn_bookmarks_coordinator(
            bookmarks.clone(),
            receiver_cancel,
            ctx,
            repo,
            warmers,
            TEST_LOOP_SLEEP,
            BookmarkUpdateDelay::Disallow,
        );
        wait_for_bookmark(
            &bookmarks,
            &failing_book,
            Some((failing_cs_id, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        let _ = cancel.send(());

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_spawn_bookmarks_coordinator_check_single_updater(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        RootUnodeManifestId::derive(
            ctx.clone(),
            repo.clone(),
            repo.get_bonsai_bookmark(ctx.clone(), &BookmarkName::new("master")?)
                .compat()
                .await?
                .unwrap(),
        )
        .compat()
        .await?;

        let derive_sleep_time_ms = 100;
        let how_many_derived = Arc::new(RwLock::new(HashMap::new()));
        let warmer = Warmer {
            warmer: Box::new({
                cloned!(how_many_derived);
                move |ctx, repo, cs_id| {
                    how_many_derived.with_write(|map| {
                        *map.entry(cs_id).or_insert(0) += 1;
                    });
                    tokio::time::delay_for(Duration::from_millis(derive_sleep_time_ms))
                        .map(|_| {
                            let res: Result<_, Error> = Ok(());
                            res
                        })
                        .compat()
                        .and_then(move |_| RootUnodeManifestId::derive(ctx, repo, cs_id).from_err())
                        .map(|_| ())
                        .boxify()
                }
            }),
            is_warm: Box::new(|ctx, repo, cs_id| {
                async move {
                    let res = RootUnodeManifestId::is_derived(&ctx, &repo, &cs_id).await?;
                    Ok(res)
                }
                .boxed()
            }),
        };
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(warmer);
        let warmers = Arc::new(warmers);

        let bookmarks = repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .compat()
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;
        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let master = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "master").set_to(master).await?;

        let (cancel, receiver_cancel) = oneshot::channel();
        spawn_bookmarks_coordinator(
            bookmarks.clone(),
            receiver_cancel,
            ctx.clone(),
            repo.clone(),
            warmers.clone(),
            TEST_LOOP_SLEEP,
            BookmarkUpdateDelay::Disallow,
        );
        // Give it a chance to derive
        wait({
            move || {
                cloned!(ctx, repo, master, warmers);
                async move {
                    let res: Result<_, Error> = Ok(is_warm(&ctx, &repo, &master, &warmers).await);
                    res
                }
            }
        })
        .await?;

        let _ = cancel.send(());

        how_many_derived.with_read(|derived| {
            assert_eq!(derived.get(&master), Some(&1));
        });

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_spawn_bookmarks_coordinator_with_publishing_bookmarks(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bookmarks = repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .compat()
            .map_ok(|(book, cs_id)| {
                let kind = *book.kind();
                (book.into_name(), (cs_id, kind))
            })
            .try_collect::<HashMap<_, _>>()
            .await?;

        let bookmarks = Arc::new(RwLock::new(bookmarks));

        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        let warmers = Arc::new(warmers);

        let new_cs_id = CreateCommitContext::new(&ctx, &repo, vec!["master"])
            .add_file("somefile", "content")
            .commit()
            .await?;
        bookmark(&ctx, &repo, "publishing")
            .create_publishing(new_cs_id)
            .await?;

        let (cancel, receiver_cancel) = oneshot::channel();
        spawn_bookmarks_coordinator(
            bookmarks.clone(),
            receiver_cancel,
            ctx.clone(),
            repo.clone(),
            warmers,
            TEST_LOOP_SLEEP,
            BookmarkUpdateDelay::Disallow,
        );

        let publishing_book = BookmarkName::new("publishing")?;
        wait_for_bookmark(
            &bookmarks,
            &publishing_book,
            Some((new_cs_id, BookmarkKind::Publishing)),
        )
        .await?;

        // Now recreate a bookmark with the same name but different kind
        bookmark(&ctx, &repo, "publishing").delete().await?;
        bookmark(&ctx, &repo, "publishing")
            .set_to(new_cs_id)
            .await?;

        wait_for_bookmark(
            &bookmarks,
            &publishing_book,
            Some((new_cs_id, BookmarkKind::PullDefaultPublishing)),
        )
        .await?;

        let _ = cancel.send(());

        Ok(())
    }
}
