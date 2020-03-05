/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Error;
use blame::BlameRoot;
use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, Freshness};
use changeset_info::ChangesetInfo;
use cloned::cloned;
use context::CoreContext;
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
use futures_stats::futures03::TimedFutureExt;
use itertools::Itertools;
use lock_ext::RwLockExt;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_types::ChangesetId;
use slog::{debug, info, warn};
use stats::prelude::*;
use time_ext::DurationExt;
use unodes::RootUnodeManifestId;

define_stats! {
    prefix = "mononoke.bookmarks.warm_bookmarks_cache";
    cached_bookmark_update_time_ms: dynamic_timeseries("{}.update_time", (repo: String); Average, Sum),
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

        let warmers = Arc::new(warmers);
        let (sender, receiver) = oneshot::channel();
        let warm_cs_ids = Arc::new(RwLock::new(HashSet::new()));

        async move {
            let bookmarks = init_bookmarks(&ctx, &repo, &warmers).await?;
            let bookmarks = Arc::new(RwLock::new(bookmarks));

            spawn_bookmarks_updater(
                bookmarks.clone(),
                receiver,
                ctx.clone(),
                repo.clone(),
                warmers.clone(),
                warm_cs_ids.clone(),
            );
            info!(ctx.logger(), "Started warm bookmark cache updater");
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

async fn move_bookmark_back_in_history_until_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    book: &BookmarkName,
    warmers: &Arc<Vec<Warmer>>,
) -> Result<Option<ChangesetId>, Error> {
    let history_depth_limits = vec![0, 10, 50, 100, 1000, 10000];

    info!(ctx.logger(), "moving {} bookmark back in history...", book);
    for (prev_limit, limit) in history_depth_limits.into_iter().tuple_windows() {
        debug!(ctx.logger(), "{} bookmark, limit {}", book, limit);
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
        let mut maybe_derived = stream::iter(log_entries
            .into_iter()
            .map(|(maybe_cs_id, _, _)| async move {
                match maybe_cs_id {
                Some(cs_id) =>  {
                    let derived = is_derived(ctx, repo, &cs_id, warmers).await;
                    (Some(cs_id), derived)
                }
                None => {
                    (None, true)
                }
            }})
        )
        // At most 100 blobstore fetches at a time
        .buffered(100);

        while let Some((maybe_cs_id, is_derived)) = maybe_derived.next().await {
            if is_derived {
                return Ok(maybe_cs_id.clone());
            }
        }

        // Bookmark has been created recently and wasn't derived at all
        if (log_entries_fetched as u32) < limit {
            return Ok(None);
        }
    }

    let cur_bookmark_value = repo.get_bonsai_bookmark(ctx.clone(), book).compat().await?;
    warn!(
        ctx.logger(),
        "cannot find previous derived version of {}, returning current version {:?}",
        book,
        cur_bookmark_value
    );
    Ok(cur_bookmark_value)
}

fn spawn_bookmarks_updater(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    terminate: oneshot::Receiver<()>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Warmer>>,
    warm_cs_ids: Arc<RwLock<HashSet<ChangesetId>>>,
) {
    // ignore JoinHandle, because we want it to run until `terminate` receives a signal
    let _ = tokio::spawn(async move {
        info!(ctx.logger(), "Starting warm bookmark cache updater");
        let infinite_loop = async {
            loop {
                let repoid = repo.get_repoid();
                let (stats, _) = update_bookmarks(&bookmarks, &ctx, &repo, &warmers, &warm_cs_ids)
                    .timed()
                    .await;

                STATS::cached_bookmark_update_time_ms.add_value(
                    stats.completion_time.as_millis_unchecked() as i64,
                    (repoid.id().to_string(),),
                );

                let _ = tokio::time::delay_for(Duration::from_millis(1000)).await;
            }
        }
        .boxed();

        let _ = select(infinite_loop, terminate).await;

        info!(ctx.logger(), "Stopped warm bookmark cache updater");
        let res: Result<_, Error> = Ok(());
        res
    });
}

async fn update_bookmarks<'a>(
    bookmarks: &'a Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    warmers: &'a Arc<Vec<Warmer>>,
    warm_cs_ids: &'a Arc<RwLock<HashSet<ChangesetId>>>,
) -> Result<(), Error> {
    let warmed_bookmarks = repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .compat()
        .map_ok({
            cloned!(warm_cs_ids);
            move |(bookmark, cs_id)| {
                let success = if warm_cs_ids.read().unwrap().contains(&cs_id) {
                    // This changeset was warmed on the previous iteration.
                    async { Ok(true) }.left_future()
                } else {
                    // Derive all the necessary data to make the changeset
                    // warm. This makes sure the read path doesn't have to
                    // generate derived data if a bookmark is requested
                    // (which is the most common case).  Ignore any errors
                    // during derivation - we don't want that to affect
                    // the set of bookmarks - but keep track of which ones
                    // succeeded, and only count successfully warmed
                    // bookmarks as warm. Changesets that fail derivation
                    // will be tried again on the next iteration if they are
                    // still bookmarked.
                    //
                    // Spawn each warmer into a separate task so that they
                    // run in parallel.
                    let mut warmers = warmers
                        .iter()
                        .map(|warmer| {
                            let join_handle = tokio::spawn(
                                (*warmer.warmer)(ctx.clone(), repo.clone(), cs_id).compat(),
                            );
                            async move {
                                let res: Result<_, Error> = match join_handle.await {
                                    Ok(Ok(_)) => Ok(true),
                                    Ok(Err(_)) | Err(_) => Ok(false),
                                };
                                res
                            }
                        })
                        .collect::<FuturesUnordered<_>>();
                    async move {
                        let mut res = true;
                        while let Some(newres) = warmers.next().await {
                            res &= newres?;
                        }
                        Ok(res)
                    }
                    .right_future()
                };

                success.map_ok(move |success| (bookmark.into_name(), cs_id, success))
            }
        })
        .try_buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;

    // The new set of warm changesets are those which were
    // already warm, or for which all derivations succeeded.
    let new_warm_cs_ids: HashSet<_> = warmed_bookmarks
        .iter()
        .filter(|(_name, _cs_id, success)| *success)
        .map(|(_name, cs_id, _success)| cs_id.clone())
        .collect();
    warm_cs_ids.with_write(|warm_cs_ids| *warm_cs_ids = new_warm_cs_ids);
    // The new bookmarks are all the bookmarks, regardless of
    // whether derivation succeeded.
    let new_bookmarks: HashMap<_, _> = warmed_bookmarks
        .into_iter()
        .map(|(name, cs_id, _success)| (name, cs_id))
        .collect();
    bookmarks.with_write(|bookmarks| *bookmarks = new_bookmarks);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobrepo::DangerousOverride;
    use blobstore::Blobstore;
    use delayblob::DelayedBlobstore;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use maplit::hashmap;
    use tests_utils::{bookmark, resolve_cs_id, CreateCommitContext};

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
}
