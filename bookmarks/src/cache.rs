// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{
    Bookmark, BookmarkHgKind, BookmarkName, BookmarkPrefix, BookmarkUpdateLogEntry,
    BookmarkUpdateReason, Bookmarks, Freshness, Transaction,
};
use context::CoreContext;
use failure::{err_msg, Error};
use failure_ext::Result;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use mononoke_types::{ChangesetId, RepositoryId, Timestamp};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type CacheData = BTreeMap<BookmarkName, (BookmarkHgKind, ChangesetId)>;

#[derive(Clone)]
struct Cache {
    expires: Instant,
    maybe_stale: bool,
    current: future::Shared<BoxFuture<CacheData, Error>>,
}

impl Cache {
    // NOTE: this function should be fast, as it is executed under a lock
    fn new(
        ctx: CoreContext,
        repoid: RepositoryId,
        bookmarks: Arc<dyn Bookmarks>,
        expires: Instant,
        maybe_stale: bool,
    ) -> Self {
        let freshness = if maybe_stale {
            Freshness::MaybeStale
        } else {
            Freshness::MostRecent
        };

        let current = future::lazy(move || {
            bookmarks
                .list_publishing_by_prefix(ctx, &BookmarkPrefix::empty(), repoid, freshness)
                .fold(
                    BTreeMap::new(),
                    |mut map, (bookmark, changeset_id)| -> Result<CacheData> {
                        let Bookmark { name, hg_kind } = bookmark;
                        map.insert(name, (hg_kind, changeset_id));
                        Ok(map)
                    },
                )
        })
        .boxify()
        .shared();

        Cache {
            expires,
            current,
            maybe_stale,
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
    ttl: Duration,
    caches: Arc<Mutex<HashMap<RepositoryId, Cache>>>,
    bookmarks: Arc<dyn Bookmarks>,
}

impl CachedBookmarks {
    pub fn new(bookmarks: Arc<dyn Bookmarks>, ttl: Duration) -> Self {
        Self {
            ttl,
            bookmarks,
            caches: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Gets or creates a cache for specified respository
    fn get_cache(&self, ctx: CoreContext, repoid: RepositoryId) -> Cache {
        let mut caches = self.caches.lock().expect("lock poisoned");
        let now = Instant::now();
        caches
            .entry(repoid)
            // create new cache if the old one has either expired or failed
            .and_modify(|cache| {
                let cache_failed = cache.is_failed();
                if cache.expires <= now || cache_failed {
                    *cache = Cache::new(
                        ctx.clone(),
                        repoid,
                        self.bookmarks.clone(),
                        now + self.ttl,
                        // NOTE: We want maybe_stale behave as follows
                        //  - if we asking for stale bookmarks we want to keep asking for
                        //    this type of bookmarks
                        //  - if we had a write from current machine, `purge_cache` will
                        //    request bookmarks from master region, but it might fail
                        //    and only in this case we want to keep asking for latest bookmarks,
                        //    in case of success, next request should go through replica
                        !cache_failed || cache.maybe_stale,
                    );
                }
            })
            // create new cache if threre is no cache entry
            .or_insert_with(|| Cache::new(ctx, repoid, self.bookmarks.clone(), now + self.ttl, true))
            .clone()
    }

    /// Removes old cache entry and replaces whith new one which will go through master region
    fn purge_cache(&self, ctx: CoreContext, repoid: RepositoryId) -> Cache {
        let cache = Cache::new(
            ctx,
            repoid,
            self.bookmarks.clone(),
            Instant::now() + self.ttl,
            /* maybe_stale */ false,
        );
        {
            let mut caches = self.caches.lock().expect("lock poisoned");
            caches.insert(repoid, cache.clone());
        }
        cache
    }

    /// Answers a bookmark query from cache.
    fn list_from_publishing_cache(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repoid: RepositoryId,
        filter: fn(&BookmarkHgKind) -> bool,
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        let range = prefix.to_range();
        let cache = self.get_cache(ctx, repoid);
        cache
            .current
            .clone()
            .map_err(|err| err_msg(err)) // unlift shared error
            .map(move |bookmarks| {
                let result: Vec<_> = bookmarks
                    .range(range)
                    .filter_map(move |(name, (hg_kind, changeset_id))| {
                        match filter(hg_kind) {
                            true => {
                                let bookmark = Bookmark { name: name.clone(), hg_kind: *hg_kind };
                                Some((bookmark, *changeset_id))
                            },
                            false => None,
                        }
                    })
                    .collect();
                stream::iter_ok(result)
            })
            .flatten_stream()
            .boxify()
    }
}

struct CachedBookmarksTransaction {
    ctx: CoreContext,
    repoid: RepositoryId,
    caches: CachedBookmarks,
    transaction: Box<dyn Transaction>,
    dirty: bool,
}

impl CachedBookmarksTransaction {
    fn new(
        ctx: CoreContext,
        repoid: RepositoryId,
        caches: CachedBookmarks,
        transaction: Box<dyn Transaction>,
    ) -> Self {
        Self {
            ctx,
            repoid,
            transaction,
            caches,
            dirty: false,
        }
    }
}

impl Bookmarks for CachedBookmarks {
    fn list_publishing_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        freshness: Freshness,
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        // Our cache only contains Publishing entries, so they all pass our filter.
        fn filter(_hg_kind: &BookmarkHgKind) -> bool {
            true
        }

        match freshness {
            Freshness::MaybeStale => self.list_from_publishing_cache(ctx, prefix, repo_id, filter),
            Freshness::MostRecent => self
                .bookmarks
                .list_publishing_by_prefix(ctx, prefix, repo_id, freshness),
        }
    }

    fn list_pull_default_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        freshness: Freshness,
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        // Our cache contains Publishing entries, but not all of their are PullDefault as well
        // (which is a subset of Publishing). So, we filter our those that aren't acceptable here.
        fn filter(hg_kind: &BookmarkHgKind) -> bool {
            use BookmarkHgKind::*;
            match hg_kind {
                Scratch => false,
                PublishingNotPullDefault => false,
                PullDefault => true,
            }
        }

        match freshness {
            Freshness::MaybeStale => self.list_from_publishing_cache(ctx, prefix, repo_id, filter),
            Freshness::MostRecent => self
                .bookmarks
                .list_pull_default_by_prefix(ctx, prefix, repo_id, freshness),
        }
    }

    fn list_all_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repo_id: RepositoryId,
        freshness: Freshness,
        max: u64,
    ) -> BoxStream<(Bookmark, ChangesetId), Error> {
        // We don't cache queries that hit all bookmarks.
        return self
            .bookmarks
            .list_all_by_prefix(ctx, prefix, repo_id, freshness, max);
    }

    fn create_transaction(&self, ctx: CoreContext, repoid: RepositoryId) -> Box<dyn Transaction> {
        Box::new(CachedBookmarksTransaction::new(
            ctx.clone(),
            repoid,
            self.clone(),
            self.bookmarks.create_transaction(ctx, repoid),
        ))
    }

    fn get(
        &self,
        ctx: CoreContext,
        bookmark: &BookmarkName,
        repoid: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        // NOTE: If you to implement a Freshness notion here and try to fetch from cache, be
        // mindful that not all bookmarks are cached, so a cache miss here does not necessarily
        // mean that the Bookmark does not exist.
        self.bookmarks.get(ctx, bookmark, repoid)
    }

    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
        self.bookmarks
            .read_next_bookmark_log_entries(ctx, id, repoid, limit)
    }

    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
        self.bookmarks
            .read_next_bookmark_log_entries_same_bookmark_and_reason(ctx, id, repoid, limit)
    }

    fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        repoid: RepositoryId,
        max_rec: u32,
    ) -> BoxStream<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error> {
        self.bookmarks
            .list_bookmark_log_entries(ctx, name, repoid, max_rec)
    }

    fn count_further_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<u64, Error> {
        self.bookmarks
            .count_further_bookmark_log_entries(ctx, id, repoid, exclude_reason)
    }

    fn count_further_bookmark_log_entries_by_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
    ) -> BoxFuture<Vec<(BookmarkUpdateReason, u64)>, Error> {
        self.bookmarks
            .count_further_bookmark_log_entries_by_reason(ctx, id, repoid)
    }

    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<Option<u64>, Error> {
        self.bookmarks
            .skip_over_bookmark_log_entries_with_reason(ctx, id, repoid, reason)
    }
}

impl Transaction for CachedBookmarksTransaction {
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

    fn update_infinitepush(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()> {
        // Infinitepush bookmarks aren't stored in the cache.
        self.transaction
            .update_infinitepush(bookmark, new_cs, old_cs)
    }

    fn create_infinitepush(&mut self, bookmark: &BookmarkName, new_cs: ChangesetId) -> Result<()> {
        // Infinitepush bookmarks aren't stored in the cache.
        self.transaction.create_infinitepush(bookmark, new_cs)
    }

    fn commit(self: Box<Self>) -> BoxFuture<bool, Error> {
        let CachedBookmarksTransaction {
            transaction,
            caches,
            repoid,
            ctx,
            dirty,
        } = *self;

        transaction
            .commit()
            .map(move |success| {
                if success && dirty {
                    caches.purge_cache(ctx, repoid);
                }
                success
            })
            .boxify()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{
        future::Either,
        stream::StreamFuture,
        sync::{mpsc, oneshot},
    };
    use maplit::hashmap;
    use mononoke_types_mocks::changesetid::{ONES_CSID, THREES_CSID, TWOS_CSID};
    use quickcheck::quickcheck;
    use std::collections::HashSet;
    use std::fmt::Debug;
    use std::iter::FromIterator;
    use tokio::{runtime::Runtime, timer::Delay};

    fn bookmark<B: AsRef<str>>(name: B) -> Bookmark {
        Bookmark::new(
            BookmarkName::new(name).unwrap(),
            BookmarkHgKind::PullDefault,
        )
    }

    #[derive(Debug, Eq, PartialEq)]
    enum Request {
        PullDefault,
        Publishing,
        All,
    }

    type MockBookmarksRequest = (
        oneshot::Sender<Result<HashMap<Bookmark, ChangesetId>>>,
        Freshness,
        Request,
    );

    struct MockBookmarks {
        sender: mpsc::UnboundedSender<MockBookmarksRequest>,
    }

    impl MockBookmarks {
        fn create() -> (Self, mpsc::UnboundedReceiver<MockBookmarksRequest>) {
            let (sender, receiver) = mpsc::unbounded();
            (Self { sender }, receiver)
        }

        fn list_impl(
            &self,
            freshness: Freshness,
            request: Request,
        ) -> BoxStream<(Bookmark, ChangesetId), Error> {
            let (send, recv) = oneshot::channel();

            self.sender
                .unbounded_send((send, freshness, request))
                .unwrap();

            recv.map_err(Error::from)
                .and_then(|result| result)
                .map(|bm| stream::iter_ok(bm))
                .flatten_stream()
                .boxify()
        }
    }

    fn create_dirty_transaction<T: Bookmarks>(
        bookmarks: &T,
        ctx: CoreContext,
        repoid: RepositoryId,
    ) -> Box<dyn Transaction> {
        let mut transaction = bookmarks.create_transaction(ctx.clone(), repoid);

        // Dirty the transaction.
        transaction
            .force_delete(
                &BookmarkName::new("".to_string()).unwrap(),
                BookmarkUpdateReason::TestMove {
                    bundle_replay_data: None,
                },
            )
            .unwrap();

        transaction
    }

    impl Bookmarks for MockBookmarks {
        fn get(
            &self,
            _ctx: CoreContext,
            _name: &BookmarkName,
            _repoid: RepositoryId,
        ) -> BoxFuture<Option<ChangesetId>, Error> {
            unimplemented!()
        }

        fn list_publishing_by_prefix(
            &self,
            _ctx: CoreContext,
            _prefix: &BookmarkPrefix,
            _repo_id: RepositoryId,
            freshness: Freshness,
        ) -> BoxStream<(Bookmark, ChangesetId), Error> {
            self.list_impl(freshness, Request::Publishing)
        }

        fn list_pull_default_by_prefix(
            &self,
            _ctx: CoreContext,
            _prefix: &BookmarkPrefix,
            _repo_id: RepositoryId,
            freshness: Freshness,
        ) -> BoxStream<(Bookmark, ChangesetId), Error> {
            self.list_impl(freshness, Request::PullDefault)
        }

        fn list_all_by_prefix(
            &self,
            _ctx: CoreContext,
            _prefix: &BookmarkPrefix,
            _repo_id: RepositoryId,
            freshness: Freshness,
            _max: u64,
        ) -> BoxStream<(Bookmark, ChangesetId), Error> {
            self.list_impl(freshness, Request::All)
        }

        fn create_transaction(
            &self,
            _ctx: CoreContext,
            _repoid: RepositoryId,
        ) -> Box<dyn Transaction> {
            Box::new(MockTransaction)
        }

        fn read_next_bookmark_log_entries(
            &self,
            _ctx: CoreContext,
            _id: u64,
            _repoid: RepositoryId,
            _limit: u64,
        ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
            unimplemented!()
        }

        fn read_next_bookmark_log_entries_same_bookmark_and_reason(
            &self,
            _ctx: CoreContext,
            _id: u64,
            _repoid: RepositoryId,
            _limit: u64,
        ) -> BoxStream<BookmarkUpdateLogEntry, Error> {
            unimplemented!()
        }

        fn list_bookmark_log_entries(
            &self,
            _ctx: CoreContext,
            _name: BookmarkName,
            _repo_id: RepositoryId,
            _max_rec: u32,
        ) -> BoxStream<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error> {
            unimplemented!()
        }

        fn count_further_bookmark_log_entries(
            &self,
            _ctx: CoreContext,
            _id: u64,
            _repoid: RepositoryId,
            _exclude_reason: Option<BookmarkUpdateReason>,
        ) -> BoxFuture<u64, Error> {
            unimplemented!()
        }

        fn count_further_bookmark_log_entries_by_reason(
            &self,
            _ctx: CoreContext,
            _id: u64,
            _repoid: RepositoryId,
        ) -> BoxFuture<Vec<(BookmarkUpdateReason, u64)>, Error> {
            unimplemented!()
        }

        fn skip_over_bookmark_log_entries_with_reason(
            &self,
            _ctx: CoreContext,
            _id: u64,
            _repoid: RepositoryId,
            _reason: BookmarkUpdateReason,
        ) -> BoxFuture<Option<u64>, Error> {
            unimplemented!()
        }
    }

    struct MockTransaction;

    impl Transaction for MockTransaction {
        fn update(
            &mut self,
            _key: &BookmarkName,
            _new_cs: ChangesetId,
            _old_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn create(
            &mut self,
            _key: &BookmarkName,
            _new_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn force_set(
            &mut self,
            _key: &BookmarkName,
            _new_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn delete(
            &mut self,
            _key: &BookmarkName,
            _old_cs: ChangesetId,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn force_delete(
            &mut self,
            _key: &BookmarkName,
            _reason: BookmarkUpdateReason,
        ) -> Result<()> {
            Ok(())
        }

        fn update_infinitepush(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
            _old_cs: ChangesetId,
        ) -> Result<()> {
            Ok(())
        }

        fn create_infinitepush(
            &mut self,
            _bookmark: &BookmarkName,
            _new_cs: ChangesetId,
        ) -> Result<()> {
            Ok(())
        }

        fn commit(self: Box<Self>) -> BoxFuture<bool, Error> {
            future::ok(true).boxify()
        }
    }

    /// next_request provides a way to advance through the stream of requests dispatched by
    /// MockBookmarks. It'll return with the next element in the stream, and _something_ that can
    /// be passed to next_request again to get the next one (and so on). This something happens to
    /// be a future that resolves to the next element and the rest of the stream. This also has an
    /// in-build timeout to report hung tests.
    fn next_request<T, E, S, F>(
        requests: F,
        rt: &mut Runtime,
        timeout_ms: u64,
    ) -> (T, StreamFuture<S>)
    where
        T: Send + 'static,
        E: Send + 'static,
        S: Stream<Item = T, Error = E> + Send + 'static,
        F: Future<Item = (Option<T>, S), Error = (E, S)> + Send + 'static,
    {
        let timeout = Duration::from_millis(timeout_ms);
        let delay = Delay::new(Instant::now() + timeout);

        match rt.block_on(delay.select2(requests)) {
            Ok(Either::A((_, _))) => panic!("no request came through!"),
            Ok(Either::B((r, _))) => {
                let (request, stream) = r;
                (request.unwrap(), stream.into_future())
            }
            _ => panic!("future errored"),
        }
    }

    fn assert_no_pending_requests<T, E, F>(fut: F, rt: &mut Runtime, timeout_ms: u64) -> F
    where
        T: Debug + Send + 'static,
        E: Send + 'static,
        F: Future<Item = T, Error = E> + Send + 'static,
    {
        let timeout = Duration::from_millis(timeout_ms);
        let delay = Delay::new(Instant::now() + timeout);

        match rt.block_on(delay.select2(fut)) {
            Ok(Either::A((_, b))) => b,
            Ok(Either::B((r, _))) => panic!("pending request was found: {:?}", r),
            _ => panic!("future errored"),
        }
    }

    #[test]
    fn test_cached_bookmarks() {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let repoid = RepositoryId::new(0);

        let (mock, requests) = MockBookmarks::create();
        let requests = requests.into_future();

        let bookmarks = CachedBookmarks::new(Arc::new(mock), Duration::from_secs(3));

        let spawn_query = |prefix: &'static str, rt: &mut Runtime| {
            let (sender, receiver) = oneshot::channel();

            let fut = bookmarks
                .list_publishing_by_prefix(
                    ctx.clone(),
                    &BookmarkPrefix::new(prefix).unwrap(),
                    repoid,
                    Freshness::MaybeStale,
                )
                .collect()
                .map(move |r| sender.send(r).unwrap())
                .discard();

            rt.spawn(fut);

            receiver
        };

        // multiple requests should create only one underlying request
        let res0 = spawn_query("a", &mut rt);
        let res1 = spawn_query("b", &mut rt);

        let ((responder, freshness, request), requests) = next_request(requests, &mut rt, 100);
        assert_eq!(freshness, Freshness::MaybeStale);
        assert_eq!(request, Request::Publishing);

        // We expect no other additional request to show up.
        let requests = assert_no_pending_requests(requests, &mut rt, 100);

        responder
            .send(Ok(hashmap! {
                bookmark("a0") => ONES_CSID,
                bookmark("b0") => TWOS_CSID,
                bookmark("b1") => THREES_CSID,
            }))
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
        let requests = assert_no_pending_requests(requests, &mut rt, 100);

        // Create a non dirty transaction and make sure that no requests go to master.
        let transaction = bookmarks.create_transaction(ctx.clone(), repoid);
        rt.block_on(transaction.commit()).unwrap();

        let _ = spawn_query("c", &mut rt);
        let requests = assert_no_pending_requests(requests, &mut rt, 100);

        // successfull transaction should redirect further requests to master
        let transaction = create_dirty_transaction(&bookmarks, ctx.clone(), repoid);
        rt.block_on(transaction.commit()).unwrap();

        let res = spawn_query("a", &mut rt);

        let ((responder, freshness, request), requests) = next_request(requests, &mut rt, 100);
        assert_eq!(freshness, Freshness::MostRecent);
        assert_eq!(request, Request::Publishing);
        responder
            .send(Err(err_msg("request to master failed")))
            .unwrap();

        rt.block_on(res).expect_err("cache did not bubble up error");

        // If request to master failed, next request should go to master too
        let res = spawn_query("a", &mut rt);

        let ((responder, freshness, request), requests) = next_request(requests, &mut rt, 100);
        assert_eq!(freshness, Freshness::MostRecent);
        assert_eq!(request, Request::Publishing);
        responder
            .send(Ok(hashmap! {
                bookmark("a") => ONES_CSID,
                bookmark("b") => TWOS_CSID,
            }))
            .unwrap();

        assert_eq!(rt.block_on(res).unwrap(), vec![(bookmark("a"), ONES_CSID)]);

        // No further requests should be made.
        let requests = assert_no_pending_requests(requests, &mut rt, 100);

        // request should be resolved with cache
        let res = spawn_query("b", &mut rt);

        assert_eq!(rt.block_on(res).unwrap(), vec![(bookmark("b"), TWOS_CSID)]);

        // No requests should have been made.
        let requests = assert_no_pending_requests(requests, &mut rt, 100);

        // cache should expire and request go to replica
        std::thread::sleep(Duration::from_secs(3));

        let res = spawn_query("b", &mut rt);

        let ((responder, freshness, request), requests) = next_request(requests, &mut rt, 100);
        assert_eq!(freshness, Freshness::MaybeStale);
        assert_eq!(request, Request::Publishing);
        responder
            .send(Ok(hashmap! {
                bookmark("b") => THREES_CSID,
            }))
            .unwrap();

        assert_eq!(
            rt.block_on(res).unwrap(),
            vec![(bookmark("b"), THREES_CSID)]
        );

        // No further requests should be made.
        let _ = assert_no_pending_requests(requests, &mut rt, 100);
    }

    fn mock_then_query(
        bookmarks: &Vec<(Bookmark, ChangesetId)>,
        query: fn(
            CachedBookmarks,
            ctx: CoreContext,
            &BookmarkPrefix,
            RepositoryId,
            Freshness,
        ) -> BoxStream<(Bookmark, ChangesetId), Error>,
        query_freshness: Freshness,
        expected_downstream_request: Request,
    ) -> HashSet<(Bookmark, ChangesetId)> {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let repo_id = RepositoryId::new(0);

        let (mock, requests) = MockBookmarks::create();
        let requests = requests.into_future();

        let store = CachedBookmarks::new(Arc::new(mock), Duration::from_secs(100));

        let (sender, receiver) = oneshot::channel();

        // Send the query to our cache. We want to use the cache, so we use MaybeStale.
        let fut = query(
            store,
            ctx,
            &BookmarkPrefix::empty(),
            repo_id,
            query_freshness,
        )
        .collect()
        .map(|r| sender.send(r).unwrap())
        .discard();
        rt.spawn(fut);

        // Wait for the underlying MockBookmarks to receive the request. We expect it to have a
        // freshness consistent with the one we send.
        let ((responder, freshness, request), _) = next_request(requests, &mut rt, 100);
        assert_eq!(freshness, query_freshness);
        assert_eq!(request, expected_downstream_request);

        // Now, dispatch the response from the Bookmarks data we have and the expected downstream
        // request we expect CachedBookmarks to have passed to its underlying MockBookmarks.
        let bookmarks = bookmarks.clone();

        let res = match request {
            Request::All => HashMap::from_iter(bookmarks),
            Request::Publishing => {
                HashMap::from_iter(bookmarks.into_iter().filter(|(b, _)| b.publishing()))
            }
            Request::PullDefault => {
                HashMap::from_iter(bookmarks.into_iter().filter(|(b, _)| b.pull_default()))
            }
        };
        responder.send(Ok(res)).unwrap();

        let out = rt.block_on(receiver).expect("query failed");
        HashSet::from_iter(out)
    }

    quickcheck! {
        fn filter_publishing(bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {
            fn query(bookmarks: CachedBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<(Bookmark, ChangesetId), Error> {
                bookmarks.list_publishing_by_prefix(ctx, prefix, repo_id, freshness)
            }

            let have = mock_then_query(&bookmarks, query, freshness, Request::Publishing);
            let want = HashSet::from_iter(bookmarks.into_iter().filter(|(b, _)| b.publishing()));
            want == have
        }

        fn filter_pull_default(bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {
            fn query(bookmarks: CachedBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<(Bookmark, ChangesetId), Error> {
                bookmarks.list_pull_default_by_prefix(ctx, prefix, repo_id, freshness)
            }

            let downstream_set = match freshness {
                // If we allow stale results, we expect CachedBookmarks to filter cached Publishing
                // bookmarks.
                Freshness::MaybeStale => Request::Publishing,
                // If we want fresh results, we expect CachedBookmarks to pass this request
                // through as-is.
                Freshness::MostRecent => Request::PullDefault,
            };

            let have = mock_then_query(&bookmarks, query, freshness, downstream_set);
            let want = HashSet::from_iter(bookmarks.into_iter().filter(|(b, _)| b.pull_default()));
            want == have
        }

        fn filter_all(bookmarks: Vec<(Bookmark, ChangesetId)>, freshness: Freshness) -> bool {
            fn query(bookmarks: CachedBookmarks, ctx: CoreContext, prefix: &BookmarkPrefix, repo_id: RepositoryId, freshness: Freshness) -> BoxStream<(Bookmark, ChangesetId), Error> {
                bookmarks.list_all_by_prefix(ctx, prefix, repo_id, freshness, std::u64::MAX)
            }

            let have = mock_then_query(&bookmarks, query, freshness, Request::All);
            let want = HashSet::from_iter(bookmarks.into_iter());
            want == have
        }
    }
}
