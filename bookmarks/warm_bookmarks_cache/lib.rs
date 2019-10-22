/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use futures::{future, stream, sync, Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use futures_stats::Timed;
use mononoke_types::ChangesetId;
use slog::{info, Logger};
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
        logger: Logger,
        repo: BlobRepo,
        warmers: Vec<Box<WarmerFn>>,
    ) -> impl Future<Item = Self, Error = Error> {
        let warmers = Arc::new(warmers);
        let bookmarks = Arc::new(RwLock::new(HashMap::new()));
        let (sender, receiver) = sync::oneshot::channel();
        spawn_bookmarks_updater(
            bookmarks.clone(),
            receiver,
            ctx.clone(),
            logger,
            repo.clone(),
            warmers.clone(),
        );
        update_bookmarks(bookmarks.clone(), ctx, repo, warmers).map(move |()| Self {
            bookmarks,
            terminate: Some(sender),
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
    terminate: sync::oneshot::Receiver<()>,
    ctx: CoreContext,
    logger: Logger,
    repo: BlobRepo,
    warmers: Arc<Vec<Box<WarmerFn>>>,
) {
    tokio::spawn(future::lazy(move || {
        info!(logger, "Starting warm bookmark cache updater");
        stream::repeat(())
            .and_then(move |()| {
                update_bookmarks(bookmarks.clone(), ctx.clone(), repo.clone(), warmers.clone())
                .timed(|stats, _| {
                    STATS::cached_bookmark_update_time_ms
                        .add_value(stats.completion_time.as_millis_unchecked() as i64);
                    Ok(())
                })
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
                info!(logger, "Stopped warm bookmark cache updater");
                Ok(())
            })
    }));
}

fn update_bookmarks(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    ctx: CoreContext,
    repo: BlobRepo,
    warmers: Arc<Vec<Box<WarmerFn>>>,
) -> impl Future<Item = (), Error = Error> {
    repo.get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map({
            move |(bookmark, cs_id)| {
                // Derive all the necessary derive data.
                // This makes sure that read path don't have to generate
                // derived data if a bookmark is requested (which is the most
                // common case). Ignore any errors during derivation - we
                // don't want that to affect the set of bookmarks.
                stream::futures_unordered(warmers.iter().map({
                    cloned!(ctx, repo);
                    move |warmer| (*warmer)(ctx.clone(), repo.clone(), cs_id).then(|_| Ok(()))
                }))
                .for_each(|_| Ok(()))
                .map(move |_| (bookmark.into_name(), cs_id))
            }
        })
        .buffered(100)
        .collect_to::<HashMap<_, _>>()
        .map({
            cloned!(bookmarks);
            move |map| {
                let mut bookmarks = bookmarks.write().unwrap();
                *bookmarks = map;
            }
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
