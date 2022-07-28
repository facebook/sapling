/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::to_hg_bookmark_stream;
use blobrepo_hg::BlobRepoHg;
use bookmarks::Bookmark;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Freshness;
use context::CoreContext;
use futures::compat::Future01CompatExt;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_01_ext::StreamExt as OldStreamExt;
use futures_ext::FbFutureExt;
use futures_ext::FbTryFutureExt;
use futures_old::Future;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mononoke_api::Repo;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tunables::tunables;
use warm_bookmarks_cache::BookmarksCache;

// We'd like to give user a consistent view of thier bookmarks for the duration of the
// whole Mononoke session. SessionBookmarkCache is used for that.
pub struct SessionBookmarkCache<R = Arc<Repo>> {
    cached_publishing_bookmarks_maybe_stale: Arc<Mutex<Option<HashMap<Bookmark, HgChangesetId>>>>,
    repo: R,
}

pub trait BookmarkCacheRepo {
    fn blobrepo(&self) -> &BlobRepo;

    fn repo_client_use_warm_bookmarks_cache(&self) -> bool;

    fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache>;
}

impl BookmarkCacheRepo for Arc<Repo> {
    fn blobrepo(&self) -> &BlobRepo {
        Repo::blob_repo(self)
    }

    fn repo_client_use_warm_bookmarks_cache(&self) -> bool {
        Repo::config(self).repo_client_use_warm_bookmarks_cache
    }

    fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache> {
        Repo::warm_bookmarks_cache(self)
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

    pub async fn get_publishing_bookmarks(
        &self,
        ctx: CoreContext,
    ) -> Result<HashMap<Bookmark, HgChangesetId>, Error> {
        let maybe_cache = {
            self.cached_publishing_bookmarks_maybe_stale
                .lock()
                .expect("lock poisoned")
                .clone()
        };

        match maybe_cache {
            None => {
                self.get_publishing_bookmarks_maybe_stale_updating_cache(ctx)
                    .await
            }
            Some(bookmarks) => Ok(bookmarks),
        }
    }

    // TODO(stash): consider updating from leader db - T70835157?
    pub fn update_publishing_bookmarks_after_push(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = (), Error = Error> + '_ {
        let cache = self.cached_publishing_bookmarks_maybe_stale.clone();
        // We just updated the bookmark, so go and fetch them from db to return
        // the newer version
        self.get_publishing_maybe_stale_from_db(ctx)
            .map(move |bookmarks| {
                update_publishing_bookmarks_maybe_stale_cache_raw(cache, bookmarks)
            })
    }

    pub async fn get_bookmarks_by_prefix(
        &self,
        ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        return_max: u64,
    ) -> Result<impl Stream<Item = Result<(BookmarkName, HgChangesetId), Error>> + '_, Error> {
        let mut kinds = vec![BookmarkKind::Scratch];

        let mut result = HashMap::new();
        if let Some(warm_bookmarks_cache) = self.get_warm_bookmark_cache() {
            let warm_bookmarks = warm_bookmarks_cache
                .list(
                    ctx,
                    prefix,
                    &BookmarkPagination::FromStart,
                    Some(return_max),
                )
                .await?;
            for (book, (cs_id, _)) in warm_bookmarks {
                result.insert(book, cs_id);
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

        Ok(to_hg_bookmark_stream(
            self.repo.blobrepo(),
            ctx,
            futures::stream::iter(result.into_iter())
                .map(Ok)
                .chain(new_bookmarks),
        ))
    }

    // Tries to fetch a bookmark from warm bookmark cache first, but if the bookmark is not found
    // then fallbacks to fetching from db.
    pub async fn get_bookmark(
        &self,
        ctx: CoreContext,
        bookmark: BookmarkName,
    ) -> Result<Option<HgChangesetId>, Error> {
        if let Some(warm_bookmarks_cache) = self.get_warm_bookmark_cache() {
            if let Some(cs_id) = warm_bookmarks_cache.get(&ctx, &bookmark).await? {
                return self
                    .repo
                    .blobrepo()
                    .derive_hg_changeset(&ctx, cs_id)
                    .map_ok(Some)
                    .await;
            }
        }

        self.repo.blobrepo().get_bookmark(ctx, &bookmark).await
    }

    async fn get_publishing_bookmarks_maybe_stale_updating_cache(
        &self,
        ctx: CoreContext,
    ) -> Result<HashMap<Bookmark, HgChangesetId>, Error> {
        let cache = self.cached_publishing_bookmarks_maybe_stale.clone();
        let bookmarks = self.get_publishing_maybe_stale_raw(ctx).await?;
        update_publishing_bookmarks_maybe_stale_cache_raw(cache, bookmarks.clone());
        Ok(bookmarks)
    }

    async fn get_publishing_maybe_stale_raw(
        &self,
        ctx: CoreContext,
    ) -> Result<HashMap<Bookmark, HgChangesetId>, Error> {
        if let Some(warm_bookmarks_cache) = self.get_warm_bookmark_cache() {
            ctx.scuba()
                .clone()
                .log_with_msg("Fetching bookmarks from Warm bookmarks cache", None);
            let s = futures_old::stream::iter_ok(
                warm_bookmarks_cache
                    .list(
                        &ctx,
                        &BookmarkPrefix::empty(),
                        &BookmarkPagination::FromStart,
                        Some(std::u64::MAX),
                    )
                    .await?
                    .into_iter()
                    .map(|(name, (cs_id, kind))| (Bookmark::new(name, kind), cs_id)),
            )
            .boxify()
            .compat();
            return to_hg_bookmark_stream(self.repo.blobrepo(), &ctx, s)
                .try_collect()
                .await;
        }
        self.get_publishing_maybe_stale_from_db(ctx).compat().await
    }

    fn get_warm_bookmark_cache(&self) -> Option<&Arc<dyn BookmarksCache>> {
        if self.repo.repo_client_use_warm_bookmarks_cache() {
            if !tunables().get_disable_repo_client_warm_bookmarks_cache() {
                return Some(self.repo.warm_bookmarks_cache());
            }
        }

        None
    }

    fn get_publishing_maybe_stale_from_db(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = HashMap<Bookmark, HgChangesetId>, Error = Error> + '_ {
        self.repo
            .blobrepo()
            .get_publishing_bookmarks_maybe_stale(ctx)
            .try_fold(HashMap::new(), |mut map, item| {
                map.insert(item.0, item.1);
                future::ready(Ok(map))
            })
            .timeout(bookmarks_timeout())
            .flatten_err()
            .boxed()
            .compat()
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
    use maplit::hashmap;
    use mononoke_api_types::InnerRepo;
    use tests_utils::bookmark;
    use tests_utils::CreateCommitContext;
    use warm_bookmarks_cache::WarmBookmarksCacheBuilder;

    struct BasicTestRepo {
        repo: BlobRepo,
        wbc: Option<Arc<dyn BookmarksCache>>,
    }

    impl BookmarkCacheRepo for BasicTestRepo {
        fn blobrepo(&self) -> &BlobRepo {
            &self.repo
        }

        fn repo_client_use_warm_bookmarks_cache(&self) -> bool {
            self.wbc.is_some()
        }

        fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache> {
            self.wbc.as_ref().unwrap()
        }
    }

    #[fbinit::test]
    async fn test_fetch_prefix_no_warm_bookmark_cache(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: InnerRepo = test_repo_factory::build_empty(fb)?;

        let cs_id = CreateCommitContext::new_root(&ctx, &repo.blob_repo)
            .add_file("1", "1")
            .commit()
            .await?;

        let hg_cs_id = repo.blob_repo.derive_hg_changeset(&ctx, cs_id).await?;
        bookmark(&ctx, &repo.blob_repo, "prefix/scratchbook")
            .create_scratch(cs_id)
            .await?;

        bookmark(&ctx, &repo.blob_repo, "prefix/publishing")
            .create_publishing(cs_id)
            .await?;

        bookmark(&ctx, &repo.blob_repo, "prefix/pulldefault")
            .create_pull_default(cs_id)
            .await?;

        // Let's try without WarmBookmarkCache first
        println!("No warm bookmark cache");
        let session_bookmark_cache = SessionBookmarkCache::new(BasicTestRepo {
            repo: repo.blob_repo.clone(),
            wbc: None,
        });
        validate(&ctx, hg_cs_id, &session_bookmark_cache).await?;

        // Let's try with WarmBookmarkCache next
        println!("With warm bookmark cache");
        let mut builder = WarmBookmarksCacheBuilder::new(ctx.clone(), &repo);
        builder.add_hg_warmers()?;
        let wbc = builder.build().await?;
        let session_bookmark_cache = SessionBookmarkCache::new(BasicTestRepo {
            repo: repo.blob_repo.clone(),
            wbc: Some(Arc::new(wbc)),
        });
        validate(&ctx, hg_cs_id, &session_bookmark_cache).await?;

        Ok(())
    }

    async fn validate(
        ctx: &CoreContext,
        hg_cs_id: HgChangesetId,
        session_bookmark_cache: &SessionBookmarkCache<BasicTestRepo>,
    ) -> Result<(), Error> {
        let maybe_hg_cs_id = session_bookmark_cache
            .get_bookmark(ctx.clone(), BookmarkName::new("prefix/scratchbook")?)
            .await?;
        assert_eq!(maybe_hg_cs_id, Some(hg_cs_id));

        let res = session_bookmark_cache
            .get_bookmarks_by_prefix(ctx, &BookmarkPrefix::new("prefix")?, 3)
            .await?
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
            .get_bookmarks_by_prefix(ctx, &BookmarkPrefix::new("prefix")?, 1)
            .await?
            .try_collect::<HashMap<_, _>>()
            .await?;
        assert_eq!(res.len(), 1);

        Ok(())
    }
}
