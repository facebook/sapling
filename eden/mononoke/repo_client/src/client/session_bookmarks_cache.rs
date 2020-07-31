/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{process_timeout_error, BOOKMARKS_TIMEOUT};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::{to_hg_bookmark_stream, BlobRepoHg};
use bookmarks::Bookmark;
use context::CoreContext;
use futures_ext::{FutureExt, StreamExt};
use futures_old::{future as future_old, Future, Stream};
use mercurial_types::HgChangesetId;
use scuba_ext::ScubaSampleBuilderExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_old::util::FutureExt as TokioFutureExt;
use tunables::tunables;
use warm_bookmarks_cache::WarmBookmarksCache;

// We'd like to give user a consistent view of thier bookmarks for the duration of the
// whole Mononoke session. SessionBookmarkCache is used for that.
pub struct SessionBookmarkCache {
    cached_publishing_bookmarks_maybe_stale: Arc<Mutex<Option<HashMap<Bookmark, HgChangesetId>>>>,
    repo: BlobRepo,
    maybe_warm_bookmarks_cache: Option<Arc<WarmBookmarksCache>>,
}

impl SessionBookmarkCache {
    pub fn new(
        repo: BlobRepo,
        maybe_warm_bookmarks_cache: Option<Arc<WarmBookmarksCache>>,
    ) -> Self {
        Self {
            cached_publishing_bookmarks_maybe_stale: Arc::new(Mutex::new(None)),
            repo,
            maybe_warm_bookmarks_cache,
        }
    }

    pub fn drop_cache(&self) {
        let _ = self
            .cached_publishing_bookmarks_maybe_stale
            .lock()
            .expect("lock poisoned")
            .take();
    }

    pub fn get_publishing_bookmarks(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        let maybe_cache = {
            self.cached_publishing_bookmarks_maybe_stale
                .lock()
                .expect("lock poisoned")
                .clone()
        };

        match maybe_cache {
            None => self
                .get_publishing_bookmarks_maybe_stale_updating_cache(ctx)
                .left_future(),
            Some(bookmarks) => future_old::ok(bookmarks).right_future(),
        }
    }

    // TODO(stash): consider updating from leader db - T70835157?
    pub fn update_publishing_bookmarks_after_push(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = (), Error = Error> {
        let cache = self.cached_publishing_bookmarks_maybe_stale.clone();
        // We just updated the bookmark, so go and fetch them from db to return
        // the newer version
        self.get_publishing_maybe_stale_from_db(ctx)
            .map(move |bookmarks| {
                update_publishing_bookmarks_maybe_stale_cache_raw(cache, bookmarks)
            })
    }

    fn get_publishing_bookmarks_maybe_stale_updating_cache(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        let cache = self.cached_publishing_bookmarks_maybe_stale.clone();
        self.get_publishing_maybe_stale_raw(ctx)
            .inspect(move |bookmarks| {
                update_publishing_bookmarks_maybe_stale_cache_raw(cache, bookmarks.clone())
            })
    }

    fn get_publishing_maybe_stale_raw(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        if let Some(warm_bookmarks_cache) = &self.maybe_warm_bookmarks_cache {
            if !tunables().get_disable_repo_client_warm_bookmarks_cache() {
                ctx.scuba()
                    .clone()
                    .log_with_msg("Fetching bookmarks from Warm bookmarks cache", None);

                let s = futures_old::stream::iter_ok(
                    warm_bookmarks_cache
                        .get_all()
                        .into_iter()
                        .map(|(name, (cs_id, kind))| (Bookmark::new(name, kind), cs_id)),
                )
                .boxify();
                return to_hg_bookmark_stream(&self.repo, &ctx, s)
                    .collect_to()
                    .left_future();
            }
        }
        self.get_publishing_maybe_stale_from_db(ctx).right_future()
    }

    fn get_publishing_maybe_stale_from_db(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        self.repo
            .get_publishing_bookmarks_maybe_stale(ctx)
            .fold(HashMap::new(), |mut map, item| {
                map.insert(item.0, item.1);
                let ret: Result<_, Error> = Ok(map);
                ret
            })
            .timeout(*BOOKMARKS_TIMEOUT)
            .map_err(process_timeout_error)
    }
}

fn update_publishing_bookmarks_maybe_stale_cache_raw(
    cache: Arc<Mutex<Option<HashMap<Bookmark, HgChangesetId>>>>,
    bookmarks: HashMap<Bookmark, HgChangesetId>,
) {
    let mut maybe_cache = cache.lock().expect("lock poisoned");
    *maybe_cache = Some(bookmarks);
}
