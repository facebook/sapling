/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{process_timeout_error, BOOKMARKS_TIMEOUT};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bookmarks::Bookmark;
use context::CoreContext;
use futures_ext::FutureExt;
use futures_old::{future as future_old, Future, Stream};
use mercurial_types::HgChangesetId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_old::util::FutureExt as TokioFutureExt;

// We'd like to give user a consistent view of thier bookmarks for the duration of the
// whole Mononoke session. SessionBookmarkCache is used for that.
pub struct SessionBookmarkCache {
    cached_publishing_bookmarks_maybe_stale: Arc<Mutex<Option<HashMap<Bookmark, HgChangesetId>>>>,
    repo: BlobRepo,
}

impl SessionBookmarkCache {
    pub fn new(repo: BlobRepo) -> Self {
        Self {
            cached_publishing_bookmarks_maybe_stale: Arc::new(Mutex::new(None)),
            repo,
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

    pub fn update_publishing_bookmarks_maybe_stale_cache(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = (), Error = Error> {
        let cache = self.cached_publishing_bookmarks_maybe_stale.clone();
        self.get_publishing_maybe_stale_raw(ctx)
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
