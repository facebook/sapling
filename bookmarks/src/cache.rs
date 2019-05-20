// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{
    BookmarkName, BookmarkPrefix, BookmarkUpdateLogEntry, BookmarkUpdateReason, Bookmarks,
    Transaction,
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

#[derive(Clone)]
struct Cache {
    expires: Instant,
    maybe_stale: bool,
    current: future::Shared<BoxFuture<BTreeMap<BookmarkName, ChangesetId>, Error>>,
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
        let current = if maybe_stale {
            future::lazy(move || {
                bookmarks
                    .list_by_prefix_maybe_stale(ctx, &BookmarkPrefix::empty(), repoid)
                    .fold(BTreeMap::new(), |mut acc, (k, v)| {
                        acc.insert(k, v);
                        Ok::<_, Error>(acc)
                    })
            })
            .left_future()
        } else {
            future::lazy(move || {
                bookmarks
                    .list_by_prefix(ctx, &BookmarkPrefix::empty(), repoid)
                    .fold(BTreeMap::new(), |mut acc, (k, v)| {
                        acc.insert(k, v);
                        Ok::<_, Error>(acc)
                    })
            })
            .right_future()
        }
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
}

struct CachedBookmarksTransaction {
    ctx: CoreContext,
    repoid: RepositoryId,
    caches: CachedBookmarks,
    transaction: Box<dyn Transaction>,
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
        }
    }
}

impl Bookmarks for CachedBookmarks {
    fn list_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repoid: RepositoryId,
    ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
        let range = prefix.to_range();
        let cache = self.get_cache(ctx, repoid);
        cache
            .current
            .clone()
            .map_err(|err| err_msg(err)) // unlift shared error
            .map(move |bookmarks| {
                let result: Vec<_> = bookmarks
                    .range(range).map(|(k, v)| (k.clone(), *v))
                    .collect();
                stream::iter_ok(result)
            })
            .flatten_stream()
            .boxify()
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
        self.bookmarks.get(ctx, bookmark, repoid)
    }

    fn list_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repoid: RepositoryId,
    ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
        self.bookmarks.list_by_prefix(ctx, prefix, repoid)
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
        self.transaction.update(bookmark, new_cs, old_cs, reason)
    }

    fn create(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.transaction.create(bookmark, new_cs, reason)
    }

    fn force_set(
        &mut self,
        bookmark: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.transaction.force_set(bookmark, new_cs, reason)
    }

    fn delete(
        &mut self,
        bookmark: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.transaction.delete(bookmark, old_cs, reason)
    }

    fn force_delete(
        &mut self,
        bookmark: &BookmarkName,
        reason: BookmarkUpdateReason,
    ) -> Result<()> {
        self.transaction.force_delete(bookmark, reason)
    }

    fn commit(self: Box<Self>) -> BoxFuture<bool, Error> {
        let CachedBookmarksTransaction {
            transaction,
            caches,
            repoid,
            ctx,
        } = *self;

        transaction
            .commit()
            .map(move |success| {
                if success {
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
    use cloned::cloned;
    use futures::{
        sync::oneshot::{channel, Sender},
        IntoFuture,
    };
    use maplit::hashmap;
    use mononoke_types_mocks::changesetid::{ONES_CSID, THREES_CSID, TWOS_CSID};
    use tokio::runtime::Runtime;

    trait MutexExt {
        type Value;

        fn with<F, O>(&self, f: F) -> O
        where
            F: FnOnce(&mut Self::Value) -> O;
    }

    impl<T> MutexExt for Mutex<T> {
        type Value = T;

        fn with<F, O>(&self, f: F) -> O
        where
            F: FnOnce(&mut Self::Value) -> O,
        {
            let mut guard = self.lock().unwrap();
            f(&mut *guard)
        }
    }

    enum Request {
        ListReplica(Sender<Result<HashMap<BookmarkName, ChangesetId>>>),
        ListMaster(Sender<Result<HashMap<BookmarkName, ChangesetId>>>),
    }

    #[derive(Clone)]
    struct MockBookmarks {
        pub requests: Arc<Mutex<Vec<Request>>>,
    }

    impl MockBookmarks {
        fn new() -> Self {
            Self {
                requests: Default::default(),
            }
        }
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

        fn list_by_prefix(
            &self,
            _ctx: CoreContext,
            _prefix: &BookmarkPrefix,
            _repoid: RepositoryId,
        ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
            let (send, recv) = channel();
            self.requests.with(|rs| rs.push(Request::ListMaster(send)));
            recv.map_err(Error::from)
                .and_then(|result| result)
                .map(|bm| stream::iter_ok(bm))
                .flatten_stream()
                .boxify()
        }

        fn list_by_prefix_maybe_stale(
            &self,
            _ctx: CoreContext,
            _prefix: &BookmarkPrefix,
            _repoid: RepositoryId,
        ) -> BoxStream<(BookmarkName, ChangesetId), Error> {
            let (send, recv) = channel();
            self.requests.with(|rs| rs.push(Request::ListReplica(send)));
            recv.map_err(Error::from)
                .and_then(|result| result)
                .map(|bm| stream::iter_ok(bm))
                .flatten_stream()
                .boxify()
        }

        fn create_transaction(
            &self,
            _ctx: CoreContext,
            _repoid: RepositoryId,
        ) -> Box<dyn Transaction> {
            Box::new(MockTransaction(self.clone()))
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

    struct MockTransaction(MockBookmarks);

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

        fn commit(self: Box<Self>) -> BoxFuture<bool, Error> {
            future::ok(true).boxify()
        }
    }

    #[test]
    fn test_cached_bookmarks() {
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let repoid = RepositoryId::new(0);

        let mock = Arc::new(MockBookmarks::new());
        let bookmarks = CachedBookmarks::new(mock.clone(), Duration::from_secs(3));

        let log: Arc<Mutex<HashMap<i32, _>>> = Default::default();
        let sleep = || std::thread::sleep(Duration::from_millis(100));

        let request = |uid: i32, prefix: &'static str| {
            bookmarks
                .list_by_prefix_maybe_stale(
                    ctx.clone(),
                    &BookmarkPrefix::new(prefix).unwrap(),
                    repoid,
                )
                .collect()
                .map({
                    cloned!(log);
                    move |result| {
                        let mut log = log.lock().unwrap();
                        log.insert(uid, result);
                    }
                })
                .discard()
        };

        // multiple requests should create only one underlying request
        runtime.spawn((request(0, "a"), request(1, "b")).into_future().discard());
        sleep();
        assert_eq!(mock.requests.with(|rs| rs.len()), 1);

        let sender = match mock.requests.with(|rs| rs.pop()).unwrap() {
            Request::ListReplica(sender) => sender,
            _ => panic!("request to replica is expected"),
        };
        sender
            .send(Ok(hashmap! {
                BookmarkName::new("a0").unwrap() => ONES_CSID,
                BookmarkName::new("b0").unwrap() => TWOS_CSID,
                BookmarkName::new("b1").unwrap() => THREES_CSID,
            }))
            .unwrap();
        sleep();

        assert_eq!(
            log.with(|log| log.drain().collect::<HashMap<_, _>>()),
            hashmap! {
                0 => vec![(BookmarkName::new("a0").unwrap(), ONES_CSID)],
                1 => vec![
                    (BookmarkName::new("b0").unwrap(), TWOS_CSID),
                    (BookmarkName::new("b1").unwrap(), THREES_CSID),
                ],
            },
        );
        assert_eq!(mock.requests.with(|rs| rs.len()), 0);

        // successfull transaction should redirect requests to master
        let transaction = bookmarks.create_transaction(ctx.clone(), repoid);
        runtime.spawn(transaction.commit().discard());
        sleep();

        runtime.spawn(request(0, "a"));
        sleep();

        let sender = match mock.requests.with(|rs| rs.pop()).unwrap() {
            Request::ListMaster(sender) => sender,
            _ => panic!("request to master is expected"),
        };
        sender
            .send(Err(err_msg("request to master failed")))
            .unwrap();
        sleep();

        // if request to master failed, next reuquest should go to master too
        runtime.spawn(request(0, "a"));
        sleep();

        let sender = match mock.requests.with(|rs| rs.pop()).unwrap() {
            Request::ListMaster(sender) => sender,
            _ => panic!("request to master is expected"),
        };
        sender
            .send(Ok(hashmap! {
                BookmarkName::new("a").unwrap() => ONES_CSID,
                BookmarkName::new("b").unwrap() => TWOS_CSID,
            }))
            .unwrap();
        sleep();

        assert_eq!(
            log.with(|log| log.drain().collect::<HashMap<_, _>>()),
            hashmap! {
                0 => vec![(BookmarkName::new("a").unwrap(), ONES_CSID)],
            },
        );
        assert_eq!(mock.requests.with(|rs| rs.len()), 0);

        // request should be resolved with cache
        runtime.spawn(request(1, "b"));
        sleep();

        assert_eq!(
            log.with(|log| log.drain().collect::<HashMap<_, _>>()),
            hashmap! {
                1 => vec![(BookmarkName::new("b").unwrap(), TWOS_CSID)],
            },
        );
        assert_eq!(mock.requests.with(|rs| rs.len()), 0);

        // cache should expire and request go to replica
        std::thread::sleep(Duration::from_secs(3));
        runtime.spawn(request(1, "b"));
        sleep();

        let sender = match mock.requests.with(|rs| rs.pop()).unwrap() {
            Request::ListReplica(sender) => sender,
            _ => panic!("request to replica is expected"),
        };
        sender
            .send(Ok(hashmap! {
                BookmarkName::new("b").unwrap() => THREES_CSID,
            }))
            .unwrap();
        sleep();

        assert_eq!(
            log.with(|log| log.drain().collect::<HashMap<_, _>>()),
            hashmap! {
                1 => vec![(BookmarkName::new("b").unwrap(), THREES_CSID)],
            },
        );
        assert_eq!(mock.requests.with(|rs| rs.len()), 0);
    }
}
