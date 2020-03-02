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
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use futures_preview::{
    channel::oneshot,
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{select, FutureExt as NewFutureExt, TryFutureExt},
    stream::{FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_stats::futures03::TimedFutureExt;
use lock_ext::RwLockExt;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_types::ChangesetId;
use slog::info;
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

fn create_warmer<D: BonsaiDerived>(ctx: &CoreContext) -> Box<WarmerFn> {
    info!(ctx.logger(), "Warming {}", D::NAME);
    let warmer: Box<WarmerFn> = Box::new(|ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| {
        D::derive(ctx, repo, cs_id)
            .map(|_| ())
            .map_err(Error::from)
            .boxify()
    });
    warmer
}

impl WarmBookmarksCache {
    pub fn new(ctx: CoreContext, repo: BlobRepo) -> impl Future<Item = Self, Error = Error> {
        let derived_data_types = &repo.get_derived_data_config().derived_data_types;
        let mut warmers: Vec<Box<WarmerFn>> = Vec::new();

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

        let warmers = Arc::new(warmers);
        let bookmarks = Arc::new(RwLock::new(HashMap::new()));
        let (sender, receiver) = oneshot::channel();
        let warm_cs_ids = Arc::new(RwLock::new(HashSet::new()));
        spawn_bookmarks_updater(
            bookmarks.clone(),
            receiver,
            ctx.clone(),
            repo.clone(),
            warmers.clone(),
            warm_cs_ids.clone(),
        );

        {
            cloned!(bookmarks, ctx);
            async move { update_bookmarks(&bookmarks, &ctx, &repo, &warmers, &warm_cs_ids).await }
        }
        .boxed()
        .compat()
        .map(move |()| {
            info!(ctx.logger(), "Started warm bookmark cache updater");
            Self {
                bookmarks,
                terminate: Some(sender),
            }
        })
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

fn spawn_bookmarks_updater(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    terminate: oneshot::Receiver<()>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Box<WarmerFn>>>,
    warm_cs_ids: Arc<RwLock<HashSet<ChangesetId>>>,
) {
    // ignore JoinHandle, because we want it to run until `terminate` receives a signal
    let _ = tokio_preview::spawn(async move {
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

                let _ = tokio_preview::time::delay_for(Duration::from_millis(1000)).await;
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
    warmers: &'a Arc<Vec<Box<WarmerFn>>>,
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
                            let join_handle = tokio_preview::spawn(
                                (*warmer)(ctx.clone(), repo.clone(), cs_id).compat(),
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
