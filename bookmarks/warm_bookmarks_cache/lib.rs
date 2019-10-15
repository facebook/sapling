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
use derived_data::BonsaiDerived;
use failure::Error;
use futures::{future, stream, Future, Stream};
use futures_ext::StreamExt;
use futures_stats::Timed;
use mononoke_types::ChangesetId;
use slog::{debug, Logger};
use stats::{define_stats, Timeseries};
use time_ext::DurationExt;
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

define_stats! {
    prefix = "mononoke.bookmarks.warm_bookmarks_cache";
    cached_bookmark_update_time_ms: timeseries(RATE, SUM),
}

#[derive(Clone)]
pub struct WarmBookmarksCache {
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
}

impl WarmBookmarksCache {
    pub fn new(
        ctx: CoreContext,
        logger: Logger,
        repo: BlobRepo,
    ) -> impl Future<Item = Self, Error = Error> {
        let bookmarks = Arc::new(RwLock::new(HashMap::new()));
        spawn_bookmarks_updater(bookmarks.clone(), ctx.clone(), logger, repo.clone());
        update_bookmarks(bookmarks.clone(), ctx, repo).map(move |()| Self { bookmarks })
    }

    pub fn get(&self, bookmark: &BookmarkName) -> Option<ChangesetId> {
        self.bookmarks.read().unwrap().get(bookmark).cloned()
    }

    pub fn get_all(&self) -> HashMap<BookmarkName, ChangesetId> {
        self.bookmarks.read().unwrap().clone()
    }
}

fn spawn_bookmarks_updater(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    ctx: CoreContext,
    logger: Logger,
    repo: BlobRepo,
) {
    tokio::spawn(future::lazy({
        cloned!(bookmarks, ctx, logger, repo);
        move || {
            stream::repeat(())
                .and_then(move |()| {
                    debug!(logger, "updating bookmark cache...");
                    update_bookmarks(bookmarks.clone(), ctx.clone(), repo.clone())
                        .timed(|stats, _| {
                            STATS::cached_bookmark_update_time_ms.add_value(
                                stats.completion_time.as_millis_unchecked() as i64
                            );
                            Ok(())
                        })
                })
                // Ignore all errors and always retry - we don't want a transient
                // failure make our bookmarks stale forever
                .then(|_| {
                    let dur = Duration::from_millis(1000);
                    tokio::timer::Delay::new(Instant::now() + dur).map_err(|_| ())
                })
                .collect()
                .map(|_| ())
                .map_err(|_| ())
        }
    }));
}

fn update_bookmarks(
    bookmarks: Arc<RwLock<HashMap<BookmarkName, ChangesetId>>>,
    ctx: CoreContext,
    repo: BlobRepo,
) -> impl Future<Item = (), Error = Error> {
    repo.get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map({
            cloned!(ctx, repo);
            move |(bookmark, cs_id)| {
                // Derive all the necessary derive data.
                // This makes sure that read path don't have to generate
                // derived data if a bookmark is requested (which is the most
                // common case).
                let unodes_derived_mapping =
                    Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
                let unodes = RootUnodeManifestId::derive(
                    ctx.clone(),
                    repo.clone(),
                    unodes_derived_mapping,
                    cs_id,
                );
                repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                    .join(unodes)
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
