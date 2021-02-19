/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::process_timeout_error;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::{to_hg_bookmark_stream, BlobRepoHg};
use bookmarks::{
    Bookmark, BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, Freshness,
};
use context::CoreContext;
use futures::{compat::Stream01CompatExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use futures_01_ext::{FutureExt, StreamExt as OldStreamExt};
use futures_old::{future as future_old, Future, Stream as OldStream};
use mercurial_types::HgChangesetId;
use mononoke_repo::MononokeRepo;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_old::util::FutureExt as TokioFutureExt;
use tunables::tunables;
use warm_bookmarks_cache::WarmBookmarksCache;

// We'd like to give user a consistent view of thier bookmarks for the duration of the
// whole Mononoke session. SessionBookmarkCache is used for that.
pub struct SessionBookmarkCache<R = MononokeRepo> {
    cached_publishing_bookmarks_maybe_stale: Arc<Mutex<Option<HashMap<Bookmark, HgChangesetId>>>>,
    repo: R,
}

pub trait BookmarkCacheRepo {
    fn blobrepo(&self) -> &BlobRepo;

    fn repo_client_use_warm_bookmarks_cache(&self) -> bool;

    fn warm_bookmarks_cache(&self) -> &WarmBookmarksCache;
}

impl BookmarkCacheRepo for MononokeRepo {
    fn blobrepo(&self) -> &BlobRepo {
        MononokeRepo::blobrepo(self)
    }

    fn repo_client_use_warm_bookmarks_cache(&self) -> bool {
        MononokeRepo::repo_client_use_warm_bookmarks_cache(self)
    }

    fn warm_bookmarks_cache(&self) -> &WarmBookmarksCache {
        MononokeRepo::warm_bookmarks_cache(self).as_ref()
    }
}

fn bookmarks_timeout() -> Duration {
    let timeout = tunables().get_repo_client_bookmarks_timeout_secs();
    if timeout > 0 {
        Duration::from_secs(timeout as u64)
    } else {
        Duration::from_secs(3 * 60)
    }
}

impl<R> SessionBookmarkCache<R>
where
    R: BookmarkCacheRepo,
{
    pub fn new(repo: R) -> Self {
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

    pub fn get_bookmarks_by_prefix(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        return_max: u64,
    ) -> impl Stream<Item = Result<(BookmarkName, HgChangesetId), Error>> {
        let mut kinds = vec![BookmarkKind::Scratch];

        let mut result = HashMap::new();
        if let Some(warm_bookmarks_cache) = self.get_warm_bookmark_cache(&ctx) {
            let warm_bookmarks = warm_bookmarks_cache.get_all();
            for (book, (cs_id, _)) in warm_bookmarks {
                let current_size: u64 = result.len().try_into().unwrap();
                if current_size >= return_max {
                    break;
                }
                if prefix.is_prefix_of(&book) {
                    result.insert(book, cs_id);
                }
            }
        } else {
            // Warm bookmark cache was disabled, so we'll need to fetch publishing
            // bookmarks from db.
            kinds.extend(BookmarkKind::ALL_PUBLISHING);
        }

        let left_to_fetch = return_max.saturating_sub(result.len().try_into().unwrap());
        let new_bookmarks = if left_to_fetch > 0 {
            self.repo
                .blobrepo()
                .bookmarks()
                .list(
                    ctx.clone(),
                    Freshness::MaybeStale,
                    prefix,
                    &kinds,
                    &BookmarkPagination::FromStart,
                    left_to_fetch,
                )
                .map_ok(|(bookmark, cs_id)| (bookmark.name, cs_id))
                .left_stream()
        } else {
            futures::stream::empty().map(Ok).right_stream()
        };

        to_hg_bookmark_stream(
            &self.repo.blobrepo(),
            &ctx,
            futures::stream::iter(result.into_iter())
                .map(Ok)
                .chain(new_bookmarks),
        )
    }

    // Tries to fetch a bookmark from warm bookmark cache first, but if the bookmark is not found
    // then fallbacks to fetching from db.
    pub async fn get_bookmark(
        &self,
        ctx: CoreContext,
        bookmark: BookmarkName,
    ) -> Result<Option<HgChangesetId>, Error> {
        if let Some(warm_bookmarks_cache) = self.get_warm_bookmark_cache(&ctx) {
            if let Some(cs_id) = warm_bookmarks_cache.get(&bookmark) {
                return self
                    .repo
                    .blobrepo()
                    .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                    .map_ok(Some)
                    .await;
            }
        }

        self.repo.blobrepo().get_bookmark(ctx, &bookmark).await
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
        if let Some(warm_bookmarks_cache) = self.get_warm_bookmark_cache(&ctx) {
            ctx.scuba()
                .clone()
                .log_with_msg("Fetching bookmarks from Warm bookmarks cache", None);
            let s = futures_old::stream::iter_ok(
                warm_bookmarks_cache
                    .get_all()
                    .into_iter()
                    .map(|(name, (cs_id, kind))| (Bookmark::new(name, kind), cs_id)),
            )
            .boxify()
            .compat();
            return to_hg_bookmark_stream(&self.repo.blobrepo(), &ctx, s)
                .try_collect()
                .compat()
                .left_future();
        }
        self.get_publishing_maybe_stale_from_db(ctx).right_future()
    }

    fn get_warm_bookmark_cache(&self, ctx: &CoreContext) -> Option<&WarmBookmarksCache> {
        if self.repo.repo_client_use_warm_bookmarks_cache() {
            let mut skip_warm_bookmark_cache =
                tunables().get_disable_repo_client_warm_bookmarks_cache();

            // We don't ever need warm bookmark cache for the external sync job
            skip_warm_bookmark_cache |= ctx.session().is_external_sync();

            if !skip_warm_bookmark_cache {
                return Some(self.repo.warm_bookmarks_cache());
            }
        }

        None
    }

    fn get_publishing_maybe_stale_from_db(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> {
        self.repo
            .blobrepo()
            .get_publishing_bookmarks_maybe_stale(ctx)
            .compat()
            .fold(HashMap::new(), |mut map, item| {
                map.insert(item.0, item.1);
                let ret: Result<_, Error> = Ok(map);
                ret
            })
            .timeout(bookmarks_timeout())
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

#[cfg(test)]
mod test {
    use super::*;
    use bookmarks::BookmarkName;
    use fbinit::FacebookInit;
    use maplit::{hashmap, hashset};
    use tests_utils::{bookmark, CreateCommitContext};
    use warm_bookmarks_cache::{
        BookmarkUpdateDelay, WarmBookmarksCache, WarmBookmarksCacheBuilder,
    };

    struct TestRepo {
        repo: BlobRepo,
        wbc: Option<WarmBookmarksCache>,
    }

    impl BookmarkCacheRepo for TestRepo {
        fn blobrepo(&self) -> &BlobRepo {
            &self.repo
        }

        fn repo_client_use_warm_bookmarks_cache(&self) -> bool {
            self.wbc.is_some()
        }

        fn warm_bookmarks_cache(&self) -> &WarmBookmarksCache {
            self.wbc.as_ref().unwrap()
        }
    }

    #[fbinit::compat_test]
    async fn test_fetch_prefix_no_warm_bookmark_cache(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo = blobrepo_factory::new_memblob_empty(None)?;

        let cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("1", "1")
            .commit()
            .await?;

        let hg_cs_id = repo
            .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
            .await?;
        bookmark(&ctx, &repo, "prefix/scratchbook")
            .create_scratch(cs_id)
            .await?;

        bookmark(&ctx, &repo, "prefix/publishing")
            .create_publishing(cs_id)
            .await?;

        bookmark(&ctx, &repo, "prefix/pulldefault")
            .create_pull_default(cs_id)
            .await?;

        // Let's try without WarmBookmarkCache first
        println!("No warm bookmark cache");
        let session_bookmark_cache = SessionBookmarkCache::new(TestRepo {
            repo: repo.clone(),
            wbc: None,
        });
        validate(&ctx, hg_cs_id, &session_bookmark_cache).await?;

        // Let's try with WarmBookmarkCache next
        println!("With warm bookmark cache");
        let mut builder = WarmBookmarksCacheBuilder::new(&ctx, &repo);
        builder.add_derived_data_warmers(&hashset! {"hgchangesets".to_string()})?;
        let wbc = builder.build(BookmarkUpdateDelay::Disallow).await?;
        let session_bookmark_cache = SessionBookmarkCache::new(TestRepo {
            repo: repo.clone(),
            wbc: Some(wbc),
        });
        validate(&ctx, hg_cs_id, &session_bookmark_cache).await?;

        Ok(())
    }

    async fn validate(
        ctx: &CoreContext,
        hg_cs_id: HgChangesetId,
        session_bookmark_cache: &SessionBookmarkCache<TestRepo>,
    ) -> Result<(), Error> {
        let maybe_hg_cs_id = session_bookmark_cache
            .get_bookmark(ctx.clone(), BookmarkName::new("prefix/scratchbook")?)
            .await?;
        assert_eq!(maybe_hg_cs_id, Some(hg_cs_id));

        let res = session_bookmark_cache
            .get_bookmarks_by_prefix(&ctx, &BookmarkPrefix::new("prefix")?, 3)
            .try_collect::<HashMap<_, _>>()
            .await?;
        assert_eq!(
            hashmap! {
                BookmarkName::new("prefix/scratchbook")? => hg_cs_id,
                BookmarkName::new("prefix/publishing")? => hg_cs_id,
                BookmarkName::new("prefix/pulldefault")? => hg_cs_id,
            },
            res
        );

        let res = session_bookmark_cache
            .get_bookmarks_by_prefix(&ctx, &BookmarkPrefix::new("prefix")?, 1)
            .try_collect::<HashMap<_, _>>()
            .await?;
        assert_eq!(res.len(), 1);

        Ok(())
    }
}
