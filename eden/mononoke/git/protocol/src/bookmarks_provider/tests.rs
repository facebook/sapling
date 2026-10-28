/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks_cache::BookmarksCache;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::Future;
use futures::task::noop_waker;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;

use super::wait_for_bookmark_move;

#[facet::container]
struct TestRepo {
    #[facet]
    bookmarks_cache: dyn BookmarksCache,
}

struct ReadyCache {
    value: Mutex<Result<Option<ChangesetId>>>,
    reads: AtomicUsize,
}

impl ReadyCache {
    fn new(value: Option<ChangesetId>) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(Ok(value)),
            reads: AtomicUsize::new(0),
        })
    }

    fn set(&self, value: Result<Option<ChangesetId>>) {
        *self.value.lock().unwrap() = value;
    }
}

#[async_trait]
impl BookmarksCache for ReadyCache {
    async fn get(
        &self,
        _ctx: &CoreContext,
        _bookmark: &BookmarkKey,
    ) -> Result<Option<ChangesetId>> {
        // An old-code regression must fail the test rather than hang its first poll.
        if self.reads.fetch_add(1, Ordering::Relaxed) >= 8 {
            bail!("cache read budget exhausted without progress");
        }
        match &*self.value.lock().unwrap() {
            Ok(value) => Ok(*value),
            Err(error) => Err(anyhow::anyhow!("{error}")),
        }
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _prefix: &BookmarkPrefix,
        _pagination: &BookmarkPagination,
        _limit: Option<u64>,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>> {
        panic!("the consistency wait must only read its bookmark");
    }

    async fn sync(&self, _ctx: &CoreContext) {
        panic!("the consistency wait must not request a global cache sync");
    }
}

#[mononoke::fbinit_test]
async fn unchanged_value_yields_on_each_read(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmark = BookmarkKey::new("heads/main")?;
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    for old in [Some(ONES_CSID), None] {
        let cache = ReadyCache::new(old);
        let repo = TestRepo {
            bookmarks_cache: cache.clone(),
        };
        let mut wait = Box::pin(wait_for_bookmark_move(&ctx, &repo, &bookmark, old));

        for reads in 1..=3 {
            assert!(wait.as_mut().poll(&mut cx).is_pending());
            assert_eq!(cache.reads.load(Ordering::Relaxed), reads);
        }
        drop(wait);
        assert_eq!(cache.reads.load(Ordering::Relaxed), 3);
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn move_create_and_delete_complete_after_cache_changes(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmark = BookmarkKey::new("heads/main")?;
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    for (old, new) in [
        (Some(ONES_CSID), Some(TWOS_CSID)),
        (None, Some(ONES_CSID)),
        (Some(ONES_CSID), None),
    ] {
        let cache = ReadyCache::new(old);
        let repo = TestRepo {
            bookmarks_cache: cache.clone(),
        };
        let mut wait = Box::pin(wait_for_bookmark_move(&ctx, &repo, &bookmark, old));
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        cache.set(Ok(new));
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        assert_eq!(cache.reads.load(Ordering::Relaxed), 2);
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn already_changed_cache_completes_without_yielding(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmark = BookmarkKey::new("heads/main")?;
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let cache = ReadyCache::new(Some(TWOS_CSID));
    let repo = TestRepo {
        bookmarks_cache: cache.clone(),
    };
    let mut wait = Box::pin(wait_for_bookmark_move(
        &ctx,
        &repo,
        &bookmark,
        Some(ONES_CSID),
    ));
    assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(cache.reads.load(Ordering::Relaxed), 1);
    Ok(())
}

#[mononoke::fbinit_test]
async fn cache_error_is_propagated_after_yield(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmark = BookmarkKey::new("heads/main")?;
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let cache = ReadyCache::new(Some(ONES_CSID));
    let repo = TestRepo {
        bookmarks_cache: cache.clone(),
    };
    let mut wait = Box::pin(wait_for_bookmark_move(
        &ctx,
        &repo,
        &bookmark,
        Some(ONES_CSID),
    ));
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    cache.set(Err(anyhow::anyhow!("injected cache failure")));
    match wait.as_mut().poll(&mut cx) {
        Poll::Ready(Err(error)) => assert_eq!(error.to_string(), "injected cache failure"),
        result => panic!("expected cache error, got {result:?}"),
    }
    assert_eq!(cache.reads.load(Ordering::Relaxed), 2);
    Ok(())
}

#[mononoke::fbinit_test]
async fn waiting_allows_updater_to_make_progress(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let bookmark = BookmarkKey::new("heads/main")?;
    let cache = ReadyCache::new(Some(ONES_CSID));
    let repo = TestRepo {
        bookmarks_cache: cache.clone(),
    };
    let wait = wait_for_bookmark_move(&ctx, &repo, &bookmark, Some(ONES_CSID));
    let updater = async {
        assert_eq!(cache.reads.load(Ordering::Relaxed), 1);
        cache.set(Ok(Some(TWOS_CSID)));
    };
    let (result, ()) = futures::join!(wait, updater);
    result?;
    assert_eq!(cache.reads.load(Ordering::Relaxed), 2);
    Ok(())
}
