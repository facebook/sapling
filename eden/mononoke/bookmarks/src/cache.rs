/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks_types::Bookmark;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkName;
use bookmarks_types::BookmarkPagination;
use bookmarks_types::BookmarkPrefix;
use bookmarks_types::Freshness;
use context::CoreContext;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use shared_error::anyhow::IntoSharedError;
use shared_error::anyhow::SharedError;
use stats::prelude::*;
use tunables::tunables;

use crate::log::BookmarkUpdateReason;
use crate::subscription::BookmarksSubscription;
use crate::transaction::BookmarkTransaction;
use crate::transaction::BookmarkTransactionHook;
use crate::Bookmarks;

define_stats! {
    prefix = "mononoke.bookmarks.cache";
    cached_bookmarks_hits: dynamic_timeseries("{}.hit", (repo: String); Rate, Sum),
    cached_bookmarks_misses: dynamic_timeseries("{}.miss", (repo: String); Rate, Sum),
}

type CacheData = BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>;

#[derive(Clone)]
struct Cache {
    expires: Instant,
    freshness: Freshness,
    current: future::Shared<BoxFuture<'static, Arc<Result<CacheData, SharedError>>>>,
}

impl Cache {
    // NOTE: this function should be fast, as it is executed under a lock
    fn new(
        ctx: CoreContext,
        bookmarks: Arc<dyn Bookmarks>,
        expires: Instant,
        freshness: Freshness,
    ) -> Self {
        let current = async move {
            Arc::new(
                bookmarks
                    .list(
                        ctx,
                        freshness,
                        &BookmarkPrefix::empty(),
                        BookmarkKind::ALL_PUBLISHING,
                        &BookmarkPagination::FromStart,
                        std::u64::MAX,
                    )
                    .try_fold(
                        BTreeMap::new(),
                        |mut map, (bookmark, changeset_id)| async move {
                            let Bookmark { name, kind } = bookmark;
                            map.insert(name, (kind, changeset_id));
                            Ok(map)
                        },
                    )
                    .await
                    .shared_error(),
            )
        }
        .boxed()
        .shared();

        Cache {
            expires,
            freshness,
            current,
        }
    }

    /// Checks if current cache contains failed result
    fn is_failed(&self) -> bool {
        match self.current.peek() {
            None => false,
            Some(result) => result.is_err(),
        }
    }
}

#[derive(Clone)]
pub struct CachedBookmarks {
    repo_id: RepositoryId,
    cache: Arc<Mutex<Option<Cache>>>,
    bookmarks: Arc<dyn Bookmarks>,
}

fn ttl() -> Option<Duration> {
    let ttl_ms = match tunables().get_bookmarks_cache_ttl_ms().try_into() {
        Ok(0) => 2000,            // 0 means default.
        Ok(duration) => duration, // Use provided duration.
        Err(_) => return None,    // Negative values mean no cache.
    };

    Some(Duration::from_millis(ttl_ms))
}

impl CachedBookmarks {
    pub fn new(bookmarks: Arc<dyn Bookmarks>, repo_id: RepositoryId) -> Self {
        Self {
            repo_id,
            bookmarks,
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Gets or creates the cache
    fn cache(&self, ctx: CoreContext, ttl: Duration) -> Cache {
        let mut cache = self.cache.lock().expect("lock poisoned");
        let now = Instant::now();
        match *cache {
            Some(ref mut cache) => {
                // create new cache if the old one has either expired or failed
                let cache_failed = cache.is_failed();
                let mut cache_hit = true;
                if cache.expires <= now || cache_failed {
                    cache_hit = false;
                    *cache = Cache::new(
                        ctx,
                        self.bookmarks.clone(),
                        now + ttl,
                        // NOTE: We want freshness to behave as follows:
                        //  - if we are asking for maybe-stale bookmarks we
                        //    want to keep asking for this type of bookmarks
                        //  - if we had a write from the current machine,
                        //    `purge` will request bookmarks from the
                        //    master region, but it might fail:
                        //    - if it fails we want to keep asking for fresh bookmarks
                        //    - if it succeeds the next request should go through a
                        //      replica
                        match (cache.freshness, cache_failed) {
                            (Freshness::MostRecent, true) => Freshness::MostRecent,
                            _ => Freshness::MaybeStale,
                        },
                    );
                }

                if cache_hit {
                    STATS::cached_bookmarks_hits.add_value(1, (self.repo_id.id().to_string(),))
                } else {
                    STATS::cached_bookmarks_misses.add_value(1, (self.repo_id.id().to_string(),))
                }

                cache.clone()
            }
            None => {
                // create new cache if there isn't one
                let new_cache = Cache::new(
                    ctx,
                    self.bookmarks.clone(),
                    now + ttl,
                    Freshness::MaybeStale,
                );
                *cache = Some(new_cache.clone());
                new_cache
            }
        }
    }

    /// Removes old cache and replaces with a new one which will go through master region
    fn purge(&self, ctx: CoreContext) -> Cache {
        let ttl = ttl().unwrap_or_else(|| Duration::from_secs(0));

        let new_cache = Cache::new(
            ctx,
            self.bookmarks.clone(),
            Instant::now() + ttl,
            Freshness::MostRecent,
        );
        let mut cache = self.cache.lock().expect("lock poisoned");
        *cache = Some(new_cache.clone());
        new_cache
    }

    /// Answers a bookmark query from cache.
    fn list_from_publishing_cache(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
        ttl: Duration,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        let range = prefix.to_range().with_pagination(pagination.clone());

        let cache = self.cache(ctx, ttl);

        let filter_kinds = if BookmarkKind::ALL_PUBLISHING
            .iter()
            .all(|kind| kinds.iter().any(|k| k == kind))
        {
            // The request is for all cached kinds, no need to filter.
            None
        } else {
            // The request is for a subset of the cached kinds.
            Some(kinds.to_vec())
        };

        cache
            .current
            .clone()
            .map(move |cache_result| match &*cache_result {
                Ok(bookmarks) => {
                    let result: Vec<_> = bookmarks
                        .range(range)
                        .filter_map(move |(name, (kind, changeset_id))| {
                            if filter_kinds
                                .as_ref()
                                .map_or(true, |kinds| kinds.iter().any(|k| k == kind))
                            {
                                let bookmark = Bookmark {
                                    name: name.clone(),
                                    kind: *kind,
                                };
                                Some(Ok((bookmark, *changeset_id)))
                            } else {
                                None
                            }
                        })
                        .take(limit as usize)
                        .collect();
                    Ok(stream::iter(result))
                }
                Err(err) => Err(Error::from(err.clone())),
            })
            .try_flatten_stream()
            .boxed()
    }
}

struct CachedBookmarksTransaction {
    ctx: CoreContext,
    cache: CachedBookmarks,
    transaction: Box<dyn BookmarkTransaction>,
    dirty: bool,
}

impl CachedBookmarksTransaction {
    fn new(
        ctx: CoreContext,
        cache: CachedBookmarks,
        transaction: Box<dyn BookmarkTransaction>,
    ) -> Self {
        Self {
            ctx,
            cache,
            transaction,
            dirty: false,
        }
    }
}

#[async_trait]
impl Bookmarks for CachedBookmarks {
    fn list(
        &self,
        ctx: CoreContext,
        freshness: Freshness,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
        if let Some(ttl) = ttl() {
            if freshness == Freshness::MaybeStale {
                if kinds
                    .iter()
                    .all(|kind| BookmarkKind::ALL_PUBLISHING.iter().any(|k| k == kind))
                {
                    // All requested kinds are supported by the cache.
                    return self
                        .list_from_publishing_cache(ctx, prefix, kinds, pagination, limit, ttl);
                }
            }
        }

        // Bypass the cache as it cannot serve this request.
        self.bookmarks
            .list(ctx, freshness, prefix, kinds, pagination, limit)
    }

    fn create_transaction(&self, ctx: CoreContext) -> Box<dyn BookmarkTransaction> {
        Box::new(CachedBookmarksTransaction::new(
            ctx.clone(),
            self.clone(),
            self.bookmarks.create_transaction(ctx),
        ))
    }

    async fn create_subscription(
        &self,
        ctx: &CoreContext,
        freshness: Freshness,
    ) -> Result<Box<dyn BookmarksSubscription>> {
        self.bookmarks.create_subscription(ctx, freshness).await
    }

    fn get(
        &self,
        ctx: CoreContext,
        bookmark: &BookmarkName,
    ) -> BoxFuture<'static, Result<Option<ChangesetId>>> {
        // NOTE: If you to implement a Freshness notion here and try to fetch from cache, be
        // mindful that not all bookmarks are cached, so a cache miss here does not necessarily
        // mean that the Bookmark does not exist.
        self.bookmarks.get(ctx, bookmark)
    }

    /// Drop this cache without kicking off a refresh right now.
    fn drop_caches(&self) {
        let mut cache = self.cache.lock().expect("lock poisoned");
        *cache = None;
    }
}

impl BookmarkTransaction for CachedBookmarksTransaction {
    fn update(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.dirty = true;
        self.transaction.update(bookmark, new_cs, old_cs, reason)
    }

    fn create(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.dirty = true;
        self.transaction.create(bookmark, new_cs, reason)
    }

    fn force_set(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.dirty = true;
        self.transaction.force_set(bookmark, new_cs, reason)
    }

    fn delete(
        &mut self,
        bookmark: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.dirty = true;
        self.transaction.delete(bookmark, old_cs, reason)
    }

    fn force_delete(
        &mut self,
        bookmark: &BookmarkName,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.dirty = true;
        self.transaction.force_delete(bookmark, reason)
    }

    fn update_scratch(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()> {
        // Scratch bookmarks aren't stored in the cache.
        self.transaction.update_scratch(bookmark, new_cs, old_cs)
    }

    fn create_scratch(&mut self, bookmark: &BookmarkName, new_cs: ChangesetId) -> Result<()> {
        // Scratch bookmarks aren't stored in the cache.
        self.transaction.create_scratch(bookmark, new_cs)
    }

    fn delete_scratch(&mut self, bookmark: &BookmarkName, old_cs: ChangesetId) -> Result<()> {
        // Scratch bookmarks aren't stored in the cache.
        self.transaction.delete_scratch(bookmark, old_cs)
    }

    fn create_publishing(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.dirty = true;
        self.transaction.create_publishing(bookmark, new_cs, reason)
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<bool>> {
        let CachedBookmarksTransaction {
            transaction,
            cache,
            ctx,
            dirty,
        } = *self;

        transaction
            .commit()
            .map_ok(move |success| {
                if success && dirty {
                    cache.purge(ctx);
                }
                success
            })
            .boxed()
    }

    fn commit_with_hook(
        self: Box<Self>,
        txn_hook: BookmarkTransactionHook,
    ) -> BoxFuture<'static, Result<bool>> {
        let CachedBookmarksTransaction {
            transaction,
            cache,
            ctx,
            dirty,
        } = *self;

        transaction
            .commit_with_hook(txn_hook)
            .map_ok(move |success| {
                if success && dirty {
                    cache.purge(ctx);
                }
                success
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ascii::AsciiString;
    use fbinit::FacebookInit;
    use futures::channel::mpsc;
    use futures::channel::oneshot;
    use futures::future::Either;
    use futures::future::Future;
    use futures::stream::Stream;
    use futures::stream::StreamFuture;
    use maplit::hashmap;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use quickcheck::quickcheck;
    use std::collections::HashSet;
    use std::fmt::Debug;
    use tokio::runtime::Runtime;
    use tunables::with_tunables_async;
    use tunables::MononokeTunables;

    fn bookmark(name: impl AsRef<str>) -> Bookmark {
        Bookmark::new(
            BookmarkName::new(name).unwrap(),
            BookmarkKind::PullDefaultPublishing,
        )
    }

    #[derive(Debug)]
    struct MockBookmarksRequest {
        response: oneshot::Sender<Result<Vec<(Bookmark, ChangesetId)>>>,
        freshness: Freshness,
        prefix: BookmarkPrefix,
        kinds: Vec<BookmarkKind>,
        pagination: BookmarkPagination,
        limit: u64,
    }

    struct MockBookmarks {
        sender: mpsc::UnboundedSender<MockBookmarksRequest>,
    }

    impl MockBookmarks {
        fn create() -> (Self, mpsc::UnboundedReceiver<MockBookmarksRequest>) {
            let (sender, receiver) = mpsc::unbounded();
            (Self { sender }, receiver)
        }
    }

    fn create_dirty_transaction(
        bookmarks: &impl Bookmarks,
        ctx: CoreContext,
    ) -> Box<dyn BookmarkTransaction> {
        let mut transaction = bookmarks.create_transaction(ctx);

        // Dirty the transaction.
        transaction
            .force_delete(
                &BookmarkName::new("").unwrap(),
                BookmarkUpdateReason::TestMove,
            )
            .unwrap();

        transaction
    }

    #[async_trait]
    impl Bookmarks for MockBookmarks {
        fn get(
            &self,
            _ctx: CoreContext,
            _name: &BookmarkName,
        ) -> BoxFuture<'static, Result<Option<ChangesetId>>> {
            unimplemented!()
        }

        async fn create_subscription(
            &self,
            _: &CoreContext,
            _: Freshness,
        ) -> Result<Box<dyn BookmarksSubscription>> {
            unimplemented!()
        }

        fn list(
            &self,
            _ctx: CoreContext,
            freshness: Freshness,
            prefix: &BookmarkPrefix,
            kinds: &[BookmarkKind],
            pagination: &BookmarkPagination,
            limit: u64,
        ) -> BoxStream<'static, Result<(Bookmark, ChangesetId)>> {
            let (send, recv) = oneshot::channel();

            self.sender
                .unbounded_send(MockBookmarksRequest {
                    response: send,
                    freshness,
                    prefix: prefix.clone(),
                    kinds: kinds.to_vec(),
                    pagination: pagination.clone(),
                    limit,
                })
                .unwrap();

            recv.map_err(Error::from)
                .and_then(|result| async move { result })
                .map_ok(|bm| stream::iter(bm.into_iter().map(Ok)))
                .try_flatten_stream()
                .boxed()
        }

        fn create_transaction(&self, _ctx: CoreContext) -> Box<dyn BookmarkTransaction> {
            Box::new(MockTransaction)
        }
    }

    struct MockTransaction;

    impl BookmarkTransaction for MockTransaction {
        fn update(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
            _old_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn create(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn force_set(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn delete(
            &mut self,
            _bookmark: &BookmarkName,
            _old_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn force_delete(
            &mut self,
            _bookmark: &BookmarkName,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn update_scratch(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
            _old_cs: ChangesetId,
        ) -> Result<()> {
            Ok(())
        }

        fn create_scratch(&mut self, _bookmark: &BookmarkName, _new_cs: ChangesetId) -> Result<()> {
            Ok(())
        }

        fn delete_scratch(&mut self, _bookmark: &BookmarkName, _old_cs: ChangesetId) -> Result<()> {
            Ok(())
        }

        fn create_publishing(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn commit(self: Box<Self>) -> BoxFuture<'static, Result<bool>> {
            future::ok(true).boxed()
        }

        fn commit_with_hook(
            self: Box<Self>,
            _txn_hook: BookmarkTransactionHook,
        ) -> BoxFuture<'static, Result<bool>> {
            unimplemented!()
        }
    }

    /// Advance through the stream of requests dispatched by MockBookmarks.
    ///
    /// Returns the next element in the stream and the stream itself, which
    /// can be passed to next_request again to get the next one (and so on).
    ///
    /// Panics if no request arrives within the timeout.
    fn next_request<T, S, F>(requests: F, rt: &Runtime, timeout_ms: u64) -> (T, StreamFuture<S>)
    where
        T: Send + 'static,
        S: Stream<Item = T> + Send + Unpin + 'static,
        F: Future<Output = (Option<T>, S)> + Send + Unpin + 'static,
    {
        rt.block_on(async move {
            let timeout = Duration::from_millis(timeout_ms);
            let delay = tokio::time::sleep(timeout);
            futures::pin_mut!(delay);

            match future::select(delay, requests).await {
                Either::Left((_, _)) => panic!("no request came through!"),
                Either::Right((r, _)) => {
                    let (request, stream) = r;
                    (request.unwrap(), stream.into_future())
                }
            }
        })
    }

    /// Check that there are no pending requests on the stream.
    ///
    /// Waits for `timeout_ms`, and panics if a request arrives during that
    /// time.
    ///
    /// Otherwise, returns the stream.
    fn assert_no_pending_requests<T, F>(fut: F, rt: &Runtime, timeout_ms: u64) -> F
    where
        T: Debug + Send + 'static,
        F: Future<Output = T> + Send + Unpin + 'static,
    {
        rt.block_on(async move {
            let timeout = Duration::from_millis(timeout_ms);
            let delay = tokio::time::sleep(timeout);
            futures::pin_mut!(delay);

            match future::select(delay, fut).await {
                Either::Left((_, b)) => b,
                Either::Right((r, _)) => panic!("pending request was found: {:?}", r),
            }
        })
    }

    #[fbinit::test]
    fn test_cached_bookmarks(fb: FacebookInit) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);

        let (mock, requests) = MockBookmarks::create();
        let requests = requests.into_future();

        let bookmarks = CachedBookmarks::new(Arc::new(mock), repo_id);

        let spawn_query = |prefix: &'static str, ttl: Option<i64>, rt: &Runtime| {
            let (sender, receiver) = oneshot::channel();

            // Tunables are read in list(), which is a sync function. We wrap this into a future to
            // make ita little more refactoring friendly.
            let bookmarks = bookmarks.clone();
            let ctx = ctx.clone();

            let fut = async move {
                let res = bookmarks
                    .list(
                        ctx.clone(),
                        Freshness::MaybeStale,
                        &BookmarkPrefix::new(prefix).unwrap(),
                        BookmarkKind::ALL_PUBLISHING,
                        &BookmarkPagination::FromStart,
                        std::u64::MAX,
                    )
                    .try_collect::<Vec<_>>()
                    .await;

                if let Ok(res) = res {
                    let _ = sender.send(res);
                }
            }
            .boxed();

            let tunables = MononokeTunables::default();
            if let Some(ttl) = ttl {
                tunables.update_ints(&hashmap! {"bookmarks_cache_ttl_ms".to_string() => ttl});
            }

            rt.spawn(with_tunables_async(tunables, fut));

            receiver
        };

        let ttl = Some(3000);

        // multiple requests should create only one underlying request
        let res0 = spawn_query("a", ttl, &rt);
        let res1 = spawn_query("b", ttl, &rt);

        let (request, requests) = next_request(requests, &rt, 100);
        assert_eq!(request.freshness, Freshness::MaybeStale);
        assert_eq!(request.kinds, BookmarkKind::ALL_PUBLISHING.to_vec());

        // We expect no other additional request to show up.
        let requests = assert_no_pending_requests(requests, &rt, 100);

        request
            .response
            .send(Ok(vec![
                (bookmark("a0"), ONES_CSID),
                (bookmark("b0"), TWOS_CSID),
                (bookmark("b1"), THREES_CSID),
            ]))
            .unwrap();

        assert_eq!(
            rt.block_on(res0).unwrap(),
            vec![(bookmark("a0"), ONES_CSID)]
        );

        assert_eq!(
            rt.block_on(res1).unwrap(),
            vec![(bookmark("b0"), TWOS_CSID), (bookmark("b1"), THREES_CSID)]
        );

        // We expect no further request to show up.
        let requests = assert_no_pending_requests(requests, &rt, 100);

        // Create a non dirty transaction and make sure that no requests go to master.
        let transaction = bookmarks.create_transaction(ctx.clone());
        rt.block_on(transaction.commit()).unwrap();

        let _ = spawn_query("c", ttl, &rt);
        let requests = assert_no_pending_requests(requests, &rt, 100);

        // successfull transaction should redirect further requests to master
        let transaction = create_dirty_transaction(&bookmarks, ctx.clone());
        rt.block_on(transaction.commit()).unwrap();

        let res = spawn_query("a", ttl, &rt);

        let (request, requests) = next_request(requests, &rt, 100);
        assert_eq!(request.freshness, Freshness::MostRecent);
        assert_eq!(request.kinds, BookmarkKind::ALL_PUBLISHING.to_vec());
        request
            .response
            .send(Err(Error::msg("request to master failed")))
            .unwrap();

        rt.block_on(res).expect_err("cache did not bubble up error");

        // If request to master failed, next request should go to master too
        let res = spawn_query("a", ttl, &rt);

        let (request, requests) = next_request(requests, &rt, 100);
        assert_eq!(request.freshness, Freshness::MostRecent);
        assert_eq!(request.kinds, BookmarkKind::ALL_PUBLISHING.to_vec());
        request
            .response
            .send(Ok(vec![
                (bookmark("a"), ONES_CSID),
                (bookmark("b"), TWOS_CSID),
            ]))
            .unwrap();

        assert_eq!(rt.block_on(res).unwrap(), vec![(bookmark("a"), ONES_CSID)]);

        // No further requests should be made.
        let requests = assert_no_pending_requests(requests, &rt, 100);

        // request should be resolved with cache
        let res = spawn_query("b", ttl, &rt);

        assert_eq!(rt.block_on(res).unwrap(), vec![(bookmark("b"), TWOS_CSID)]);

        // No requests should have been made.
        let requests = assert_no_pending_requests(requests, &rt, 100);

        // cache should expire and request go to replica
        std::thread::sleep(Duration::from_secs(3));

        let res = spawn_query("b", ttl, &rt);

        let (request, requests) = next_request(requests, &rt, 100);
        assert_eq!(request.freshness, Freshness::MaybeStale);
        assert_eq!(request.kinds, BookmarkKind::ALL_PUBLISHING.to_vec());
        request
            .response
            .send(Ok(vec![(bookmark("b"), THREES_CSID)]))
            .unwrap();

        assert_eq!(
            rt.block_on(res).unwrap(),
            vec![(bookmark("b"), THREES_CSID)]
        );

        // No further requests should be made.
        let requests = assert_no_pending_requests(requests, &rt, 100);

        // Spawn two queries, but without the cache being turned on. We expect 2 requests.
        let _ = spawn_query("a", Some(-1), &rt);
        let (request, requests) = next_request(requests, &rt, 100);
        assert_eq!(request.prefix, BookmarkPrefix::new("a").unwrap());

        let _ = spawn_query("b", Some(-1), &rt);
        let (request, requests) = next_request(requests, &rt, 100);
        assert_eq!(request.prefix, BookmarkPrefix::new("b").unwrap());

        let _ = requests;
    }

    fn mock_bookmarks_response(
        bookmarks: &BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
        prefix: &BookmarkPrefix,
        kinds: &[BookmarkKind],
        pagination: &BookmarkPagination,
        limit: u64,
    ) -> Vec<(Bookmark, ChangesetId)> {
        let range = prefix.to_range().with_pagination(pagination.clone());
        bookmarks
            .range(range)
            .filter_map(|(bookmark, (kind, changeset_id))| {
                if kinds.iter().any(|k| kind == k) {
                    let bookmark = Bookmark {
                        name: bookmark.clone(),
                        kind: *kind,
                    };
                    Some((bookmark, *changeset_id))
                } else {
                    None
                }
            })
            .take(limit as usize)
            .collect()
    }

    fn mock_then_query(
        fb: FacebookInit,
        bookmarks: &BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
        query_freshness: Freshness,
        query_prefix: &BookmarkPrefix,
        query_kinds: &[BookmarkKind],
        query_pagination: &BookmarkPagination,
        query_limit: u64,
    ) -> Vec<(Bookmark, ChangesetId)> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);

        let (mock, requests) = MockBookmarks::create();
        let requests = requests.into_future();

        let store = CachedBookmarks::new(Arc::new(mock), repo_id);

        let (sender, receiver) = oneshot::channel();

        let tunables = MononokeTunables::default();
        tunables.update_ints(&hashmap! {"bookmarks_cache_ttl_ms".to_string() => 100_000});

        // Send the query to our cache.
        let fut = store
            .list(
                ctx,
                query_freshness,
                query_prefix,
                query_kinds,
                query_pagination,
                query_limit,
            )
            .try_collect()
            .map_ok(|r: Vec<_>| sender.send(r).unwrap());
        rt.spawn(with_tunables_async(tunables, fut));

        // Wait for the underlying MockBookmarks to receive the request. We
        // expect it to have a freshness consistent with the one we send.
        let (request, _) = next_request(requests, &rt, 100);
        assert_eq!(request.freshness, query_freshness);

        // Now dispatch the response from the Bookmarks data we have and the
        // expected downstream request we expect CachedBookmarks to have
        // passed to its underlying MockBookmarks.
        let response = mock_bookmarks_response(
            bookmarks,
            &request.prefix,
            request.kinds.as_slice(),
            &request.pagination,
            request.limit,
        );
        request.response.send(Ok(response)).unwrap();

        rt.block_on(receiver).expect("query failed")
    }

    quickcheck! {
        fn responses_match(
            fb: FacebookInit,
            bookmarks: BTreeMap<BookmarkName, (BookmarkKind, ChangesetId)>,
            freshness: Freshness,
            kinds: HashSet<BookmarkKind>,
            prefix_char: Option<ascii_ext::AsciiChar>,
            after: Option<BookmarkName>,
            limit: u64
        ) -> bool {
            // Test that requesting via the cache gives the same result
            // as going directly to the back-end.
            let kinds: Vec<_> = kinds.into_iter().collect();
            let prefix = match prefix_char {
                Some(ch) => BookmarkPrefix::new_ascii(AsciiString::from(&[ch.0][..])),
                None => BookmarkPrefix::empty(),
            };
            let pagination = match after {
                Some(name) => BookmarkPagination::After(name),
                None => BookmarkPagination::FromStart,
            };
            let have = mock_then_query(
                fb,
                &bookmarks,
                freshness,
                &prefix,
                kinds.as_slice(),
                &pagination,
                limit,
            );
            let want = mock_bookmarks_response(
                &bookmarks,
                &prefix,
                kinds.as_slice(),
                &pagination,
                limit,
            );
            have == want
        }
    }
}
