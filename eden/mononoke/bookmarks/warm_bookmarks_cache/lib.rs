/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Error;
use blame::BlameRoot;
use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, Freshness};
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::{
    channel::oneshot,
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, select, FutureExt as NewFutureExt, TryFutureExt},
    stream::{self, FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::Future;
use itertools::Itertools;
use lock_ext::RwLockExt;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_types::{ChangesetId, Timestamp};
use slog::{debug, info, warn};
use stats::prelude::*;
use unodes::RootUnodeManifestId;

define_stats! {
    prefix = "mononoke.warm_bookmarks_cache";
    bookmarks_fetch_failures: timeseries(Rate, Sum),
    bookmark_update_failures: timeseries(Rate, Sum),
    max_staleness_secs: dynamic_singleton_counter("{}.max_staleness_secs", (reponame: String)),
}

pub struct WarmBookmarksCache {
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    terminate: Option<oneshot::Sender<()>>,
}

pub type WarmerFn =
    dyn Fn(CoreContext, BlobRepo, ChangesetId) -> BoxFuture<(), Error> + Send + Sync + 'static;

pub type IsDerivedFn =
    dyn Fn(&CoreContext, &BlobRepo, &ChangesetId) -> BoxFuture<bool, Error> + Send + Sync + 'static;

struct Warmer {
    warmer: Box<WarmerFn>,
    is_derived: Box<IsDerivedFn>,
}

fn create_warmer<D: BonsaiDerived>(ctx: &CoreContext) -> Warmer {
    info!(ctx.logger(), "Warming {}", D::NAME);
    let warmer: Box<WarmerFn> = Box::new(|ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| {
        D::derive(ctx, repo, cs_id)
            .map(|_| ())
            .map_err(Error::from)
            .boxify()
    });

    let is_derived: Box<IsDerivedFn> =
        Box::new(|ctx: &CoreContext, repo: &BlobRepo, cs_id: &ChangesetId| {
            D::is_derived(ctx, repo, cs_id)
                .map_err(Error::from)
                .boxify()
        });
    Warmer { warmer, is_derived }
}

impl WarmBookmarksCache {
    pub fn new(ctx: CoreContext, repo: BlobRepo) -> impl Future<Item = Self, Error = Error> {
        let derived_data_types = &repo.get_derived_data_config().derived_data_types;
        let mut warmers: Vec<Warmer> = Vec::new();

        warmers.push(create_warmer::<MappedHgChangesetId>(&ctx));

        if derived_data_types.contains(RootUnodeManifestId::NAME) {
            warmers.push(create_warmer::<RootUnodeManifestId>(&ctx));
        }
        if derived_data_types.contains(RootFsnodeId::NAME) {
            warmers.push(create_warmer::<RootFsnodeId>(&ctx));
        }
        if derived_data_types.contains(BlameRoot::NAME) {
            warmers.push(create_warmer::<BlameRoot>(&ctx));
        }
        if derived_data_types.contains(ChangesetInfo::NAME) {
            warmers.push(create_warmer::<ChangesetInfo>(&ctx));
        }
        if derived_data_types.contains(RootDeletedManifestId::NAME) {
            warmers.push(create_warmer::<RootDeletedManifestId>(&ctx));
        }

        let warmers = Arc::new(warmers);
        let (sender, receiver) = oneshot::channel();

        async move {
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
            );
            Ok(Self {
                bookmarks,
                terminate: Some(sender),
            })
        }
        .boxed()
        .compat()
    }

    pub fn get(&self, bookmark: &BookmarkName) -> Option<ChangesetId> {
        self.bookmarks.read().unwrap().get(bookmark).cloned()
    }

    pub fn get_all(&self) -> HashMap<BookmarkName, ChangesetId> {
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
) -> Result<HashMap<BookmarkName, ChangesetId>, Error> {
    let all_bookmarks = repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .compat()
        .try_collect::<HashMap<_, _>>()
        .await?;

    all_bookmarks
        .into_iter()
        .map(|(book, cs_id)| async move {
            if !is_derived(ctx, repo, &cs_id, warmers).await {
                let book_name = book.into_name();
                let maybe_cs_id =
                    move_bookmark_back_in_history_until_derived(&ctx, &repo, &book_name, &warmers)
                        .await?;

                info!(
                    ctx.logger(),
                    "moved {} back in history to {:?}", book_name, maybe_cs_id
                );
                Ok(maybe_cs_id.map(|cs_id| (book_name, cs_id)))
            } else {
                Ok(Some((book.into_name(), cs_id)))
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_filter_map(|x| async { Ok(x) })
        .try_collect::<HashMap<_, _>>()
        .await
}

async fn is_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: &ChangesetId,
    warmers: &Arc<Vec<Warmer>>,
) -> bool {
    let is_derived = warmers
        .iter()
        .map(|warmer| {
            (*warmer.is_derived)(ctx, repo, cs_id)
                .compat()
                .map(|res| res.unwrap_or(false))
        })
        .collect::<FuturesUnordered<_>>();

    is_derived
        .fold(true, |acc, is_derived| future::ready(acc & is_derived))
        .await
}

async fn derive_all(
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
        LatestDerivedBookmarkEntry::Found(maybe_cs_id) => Ok(maybe_cs_id),
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

enum LatestDerivedBookmarkEntry {
    Found(Option<ChangesetId>),
    /// Latest derived bookmark entry is too far away
    NotFound,
}

/// Searches bookmark log for latest entry for which everything is derived. Note that we consider log entry that
/// deletes a bookmark to be derived. Returns this entry if it was found and changesets for all underived entries after that
/// OLDEST ENTRIES FIRST.
async fn find_all_underived_and_latest_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    book: &BookmarkName,
    warmers: &Arc<Vec<Warmer>>,
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
                        let derived = is_derived(ctx, repo, &cs_id, warmers).await;
                        (Some((cs_id, ts)), derived)
                    }
                    None => (None, true),
                }
            },
        ))
        .buffered(100);

        while let Some((maybe_cs_and_ts, is_derived)) = maybe_derived.next().await {
            if is_derived {
                let maybe_cs = maybe_cs_and_ts.map(|(cs, _)| cs);
                return Ok((LatestDerivedBookmarkEntry::Found(maybe_cs), res));
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
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    terminate: oneshot::Receiver<()>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Warmer>>,
    loop_sleep: Duration,
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
                    .map_ok(|(book, cs_id)| (book.into_name(), cs_id))
                    .try_collect::<HashMap<_, _>>()
                    .await;

                let new_bookmarks = match res {
                    Ok(bookmarks) => bookmarks,
                    Err(err) => {
                        STATS::bookmarks_fetch_failures.add_value(1);
                        warn!(ctx.logger(), "failed to fetch bookmarks {}", err);
                        continue;
                    }
                };

                let mut changed_bookmarks = vec![];
                // Find bookmarks that were moved/created and spawn an updater
                // for them
                for (key, value) in &new_bookmarks {
                    if cur_bookmarks.get(key) != Some(value) {
                        changed_bookmarks.push(key);
                    }
                }

                // Find bookmarks that were deleted
                for key in cur_bookmarks.keys() {
                    if !new_bookmarks.contains_key(&key) {
                        changed_bookmarks.push(key);
                    }
                }

                for book in changed_bookmarks {
                    let need_spawning = live_updaters.with_write(|live_updaters| {
                        if !live_updaters.contains_key(&book) {
                            live_updaters.insert(book.clone(), BookmarkUpdaterState::Started);
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
                        cloned!(ctx, repo, book, bookmarks, live_updaters, warmers);
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
                                            book.clone(),
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
                                warn!(ctx.logger(), "update of {} failed: {}", book, err);
                            };

                            live_updaters.with_write(|live_updaters| {
                                let maybe_state = live_updaters.remove(&book);
                                if let Some(state) = maybe_state {
                                    live_updaters.insert(book.clone(), state.into_finished(&res));
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
    bookmark: &BookmarkName,
    bookmarks: &Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    warmers: &Arc<Vec<Warmer>>,
    mut staleness_reporter: impl FnMut(Timestamp),
) -> Result<(), Error> {
    let (latest_derived, underived_history) =
        find_all_underived_and_latest_derived(&ctx, &repo, &bookmark, &warmers).await?;

    match latest_derived {
        // Move bookmark to the latest derived commit or delete the bookmark completely
        LatestDerivedBookmarkEntry::Found(maybe_cs_id) => match maybe_cs_id {
            Some(cs_id) => {
                bookmarks.with_write(|bookmarks| bookmarks.insert(bookmark.clone(), cs_id));
            }
            None => {
                bookmarks.with_write(|bookmarks| bookmarks.remove(&bookmark));
            }
        },
        LatestDerivedBookmarkEntry::NotFound => {
            warn!(
                ctx.logger(),
                "Haven't found previous derived version of {}! Will try to derive anyway", bookmark
            );
        }
    }

    for (underived_cs_id, ts) in underived_history {
        staleness_reporter(ts);
        let res = derive_all(&ctx, &repo, &underived_cs_id, &warmers).await;
        match res {
            Ok(()) => {
                bookmarks
                    .with_write(|bookmarks| bookmarks.insert(bookmark.clone(), underived_cs_id));
            }
            Err(err) => {
                warn!(
                    ctx.logger(),
                    "failed to derive data for {} while updating {}: {}",
                    underived_cs_id,
                    bookmark,
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
    use blobrepo::DangerousOverride;
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
            hashmap! {BookmarkName::new("master")? => master_cs_id}
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
            hashmap! {BookmarkName::new("master")? => derived_master}
        );

        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), master)
            .compat()
            .await?;
        let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
        assert_eq!(bookmarks, hashmap! {BookmarkName::new("master")? => master});

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
            hashmap! {BookmarkName::new("master")? => derived_master}
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
            hashmap! {BookmarkName::new("master")? => derived_master}
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
            .map_ok(|(book, cs_id)| (book.into_name(), cs_id))
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
        );

        let master_book = BookmarkName::new("master")?;
        wait_for_bookmark(&bookmarks, &master_book, Some(master)).await?;

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
            .map_ok(|(book, cs_id)| (book.into_name(), cs_id))
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

        let master_book = BookmarkName::new("master")?;
        single_bookmark_updater(&ctx, &repo, &master_book, &bookmarks, &warmers, |_| {}).await?;

        assert_eq!(
            bookmarks.with_read(|bookmarks| bookmarks.get(&master_book).cloned()),
            Some(master_cs_id)
        );

        Ok(())
    }

    async fn wait_for_bookmark(
        bookmarks: &Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
        book: &BookmarkName,
        expected_value: Option<ChangesetId>,
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
            .map_ok(|(book, cs_id)| (book.into_name(), cs_id))
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
            is_derived: Box::new(|ctx, repo, cs_id| {
                RootUnodeManifestId::is_derived(ctx, repo, cs_id)
                    .from_err()
                    .boxify()
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
        );

        let master_book = BookmarkName::new("master")?;
        wait_for_bookmark(&bookmarks, &master_book, Some(master)).await?;
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
        );
        wait_for_bookmark(&bookmarks, &failing_book, Some(failing_cs_id)).await?;

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
            is_derived: Box::new(|ctx, repo, cs_id| {
                RootUnodeManifestId::is_derived(ctx, repo, cs_id)
                    .from_err()
                    .boxify()
            }),
        };
        let mut warmers: Vec<Warmer> = Vec::new();
        warmers.push(warmer);
        let warmers = Arc::new(warmers);

        let bookmarks = repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .compat()
            .map_ok(|(book, cs_id)| (book.into_name(), cs_id))
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
        );
        // Give it a chance to derive
        wait({
            move || {
                cloned!(ctx, repo, master, warmers);
                async move {
                    let res: Result<_, Error> =
                        Ok(is_derived(&ctx, &repo, &master, &warmers).await);
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
}
