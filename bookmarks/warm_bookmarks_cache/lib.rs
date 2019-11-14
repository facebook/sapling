/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use futures::{future, stream, sync, Future, Stream};
use futures_ext::{spawn_future, BoxFuture, FutureExt, StreamExt};
use futures_stats::Timed;
use lock_ext::RwLockExt;
use mononoke_types::ChangesetId;
use slog::info;
use stats::{define_stats, Timeseries};
use time_ext::DurationExt;

define_stats! {
    prefix = "mononoke.bookmarks.warm_bookmarks_cache";
    cached_bookmark_update_time_ms: timeseries(RATE, SUM),
}

pub struct WarmBookmarksCache {
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    terminate: Option<sync::oneshot::Sender<()>>,
}

type WarmerFn =
    dyn Fn(CoreContext, BlobRepo, ChangesetId) -> BoxFuture<(), Error> + Send + Sync + 'static;

impl WarmBookmarksCache {
    pub fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        warmers: Vec<Box<WarmerFn>>,
    ) -> impl Future<Item = Self, Error = Error> {
        let warmers = Arc::new(warmers);
        let bookmarks = Arc::new(RwLock::new(HashMap::new()));
        let (sender, receiver) = sync::oneshot::channel();
        let warm_cs_ids = Arc::new(RwLock::new(HashSet::new()));
        spawn_bookmarks_updater(
            bookmarks.clone(),
            receiver,
            ctx.clone(),
            repo.clone(),
            warmers.clone(),
            warm_cs_ids.clone(),
        );
        update_bookmarks(bookmarks.clone(), ctx.clone(), repo, warmers, warm_cs_ids).map(
            move |()| {
                info!(ctx.logger(), "Started warm bookmark cache updater");
                Self {
                    bookmarks,
                    terminate: Some(sender),
                }
            },
        )
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
    terminate: sync::oneshot::Receiver<()>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Box<WarmerFn>>>,
    warm_cs_ids: Arc<RwLock<HashSet<ChangesetId>>>,
) {
    tokio::spawn(future::lazy(move || {
        info!(ctx.logger(), "Starting warm bookmark cache updater");
        stream::repeat(())
            .and_then({
                cloned!(ctx);
                move |()| {
                    update_bookmarks(bookmarks.clone(), ctx.clone(), repo.clone(), warmers.clone(), warm_cs_ids.clone())
                    .timed(|stats, _| {
                        STATS::cached_bookmark_update_time_ms
                            .add_value(stats.completion_time.as_millis_unchecked() as i64);
                        Ok(())
                    })
                }
            })
            .then(|_| {
                let dur = Duration::from_millis(1000);
                tokio::timer::Delay::new(Instant::now() + dur)
            })
            // Ignore all errors and always retry - we don't want a transient
            // failure make our bookmarks stale forever
            .then(|_| Ok(()))
            .for_each(|_| -> Result<(), ()> { Ok(()) })
            .select2(terminate)
            .then(move |_| {
                info!(ctx.logger(), "Stopped warm bookmark cache updater");
                Ok(())
            })
    }));
}

fn update_bookmarks(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Box<WarmerFn>>>,
    warm_cs_ids: Arc<RwLock<HashSet<ChangesetId>>>,
) -> impl Future<Item = (), Error = Error> {
    repo.get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map({
            cloned!(warm_cs_ids);
            move |(bookmark, cs_id)| {
                if warm_cs_ids.read().unwrap().contains(&cs_id) {
                    // This changeset was warmed on the previous iteration.
                    future::ok(true).left_future()
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
                    let warmers = warmers.iter().map({
                        cloned!(ctx, repo);
                        move |warmer| {
                            spawn_future((*warmer)(ctx.clone(), repo.clone(), cs_id))
                                .then(|res| Ok(res.is_ok()))
                        }
                    });
                    stream::futures_unordered(warmers)
                        .fold(true, |a, b| -> Result<bool, Error> { Ok(a && b) })
                        .right_future()
                }
                .map(move |success| (bookmark.into_name(), cs_id, success))
            }
        })
        .buffered(100)
        .collect_to::<Vec<_>>()
        .map(move |warmed_bookmarks| {
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
        })
}

/// Warm the Mecurial derived data for a changeset.
// TODO(mbthomas): move to Mercurial derived data crate when Mercurial is
// derived using the normal derivation mechanism.
pub fn warm_hg_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> BoxFuture<(), Error> {
    repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .map(|_| ())
        .boxify()
}
