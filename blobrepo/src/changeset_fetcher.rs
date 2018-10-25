// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use cachelib;
use changesets::{deserialize_cs_entries, get_cache_key, ChangesetEntry, Changesets};
use failure::{err_msg, Error};
use futures::{future, Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::RepositoryId;
use mononoke_types::{ChangesetId, Generation};

use std::collections::HashSet;
use std::sync::{Arc, Mutex, atomic::AtomicUsize, atomic::Ordering};

/// Trait that knows how to fetch DAG info about commits. Primary user is revsets
/// Concrete implementation may add more efficient caching logic to make request faster
pub trait ChangesetFetcher: Send + Sync {
    fn get_generation_number(&self, cs_id: ChangesetId) -> BoxFuture<Generation, Error>;

    fn get_parents(&self, cs_id: ChangesetId) -> BoxFuture<Vec<ChangesetId>, Error>;
}

/// Simplest ChangesetFetcher implementation which is just a wrapper around `Changesets` object
pub struct SimpleChangesetFetcher {
    changesets: Arc<Changesets>,
    repo_id: RepositoryId,
}

impl SimpleChangesetFetcher {
    pub fn new(changesets: Arc<Changesets>, repo_id: RepositoryId) -> Self {
        Self {
            changesets,
            repo_id,
        }
    }
}

impl ChangesetFetcher for SimpleChangesetFetcher {
    fn get_generation_number(&self, cs_id: ChangesetId) -> BoxFuture<Generation, Error> {
        self.changesets
            .get(self.repo_id.clone(), cs_id.clone())
            .and_then(move |maybe_cs| {
                maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id)))
            })
            .map(|cs| Generation::new(cs.gen))
            .boxify()
    }

    fn get_parents(&self, cs_id: ChangesetId) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.changesets
            .get(self.repo_id.clone(), cs_id.clone())
            .and_then(move |maybe_cs| {
                maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id)))
            })
            .map(|cs| cs.parents)
            .boxify()
    }
}

/// CachingChangesetFetcher is a special kind of ChangesetFetcher that more efficiently works
/// with cache in case of long graph traversals.
///
/// If there are too many cache misses then CachingChangesetFetcher fetches bulks of "nearby" commits
/// from blobstore, decodes them and puts them in cachelib cache. "Nearby" here means that commits
/// have close generation numbers (see `cache_bucket_size`). So in one blobstore fetch we can fetch many
/// thousands of commits and put them in the cache so that the next changeset fetches will hit
/// the cache.
///
/// Note that we won't go to blobstore if there were not many cache misses (see `cache_misses_limit`).
/// This is done intentionally to avoid slow downs for simple cases where we don't traverse much
/// of the graph.
///
/// The blobstore blobs with commits should be created by a separate process.
/// Note that blobstore blob *MUST* be immutable. If indexes are updated, for example, by a
/// separate process, then new blobs must be created.
///
/// Failure to fetch anything from the blobstore won't result in a failure
#[derive(Clone)]
pub struct CachingChangesetFetcher {
    changesets: Arc<Changesets>,
    repo_id: RepositoryId,
    cache_pool: cachelib::LruCachePool,
    cache_misses: Arc<AtomicUsize>,
    blobstore: Arc<Blobstore>,
    already_fetched_blobs: Arc<Mutex<HashSet<String>>>,
    cache_misses_limit: usize,
    cache_bucket_size: u64,
}

impl CachingChangesetFetcher {
    pub fn new(
        changesets: Arc<Changesets>,
        repo_id: RepositoryId,
        cache_pool: cachelib::LruCachePool,
        blobstore: Arc<Blobstore>,
    ) -> Self {
        Self {
            changesets,
            repo_id,
            cache_pool,
            cache_misses: Arc::new(AtomicUsize::new(0)),
            blobstore,
            already_fetched_blobs: Arc::new(Mutex::new(HashSet::new())),
            cache_misses_limit: 10000,
            cache_bucket_size: 10000,
        }
    }

    #[cfg(test)]
    pub fn new_with_opts(
        changesets: Arc<Changesets>,
        repo_id: RepositoryId,
        cache_pool: cachelib::LruCachePool,
        blobstore: Arc<Blobstore>,
        cache_misses_limit: usize,
        cache_bucket_size: u64,
    ) -> Self {
        Self {
            changesets,
            repo_id,
            cache_pool,
            cache_misses: Arc::new(AtomicUsize::new(0)),
            blobstore,
            already_fetched_blobs: Arc::new(Mutex::new(HashSet::new())),
            cache_misses_limit,
            cache_bucket_size,
        }
    }

    fn too_many_cache_misses(&self) -> bool {
        self.cache_misses.load(Ordering::Relaxed) > self.cache_misses_limit
    }

    fn get_blobstore_cache_key(&self, gen_num: u64) -> String {
        let mut bucket = gen_num / self.cache_bucket_size;
        if gen_num % self.cache_bucket_size != 0 {
            bucket += 1;
        }

        // TODO(stash): T34340486 - read blobstore key from the db
        format!("changesetscache_{}", bucket * self.cache_bucket_size)
    }

    fn fill_cache(&self, gen_num: u64) -> impl Future<Item = (), Error = Error> {
        let blobstore_cache_key = self.get_blobstore_cache_key(gen_num);
        if !self.already_fetched_blobs
            .lock()
            .unwrap()
            .contains(&blobstore_cache_key)
        {
            self.blobstore
                .get(blobstore_cache_key.clone())
                .map({
                    let cs_fetcher = self.clone();
                    move |val| {
                        cs_fetcher.already_fetched_blobs.lock().unwrap().insert(blobstore_cache_key);
                        if let Some(bytes) = val {
                            let _ = deserialize_cs_entries(bytes.as_bytes()).map(|entries| {
                                for entry in entries {
                                    let cachelib_cache_key = get_cache_key(
                                        &entry.repo_id,
                                        &entry.cs_id
                                    );
                                    let _ = cachelib::set_cached(
                                        &cs_fetcher.cache_pool,
                                        &cachelib_cache_key,
                                        &entry,
                                    );
                                }
                            });
                        }
                }})
                // Ignore errors since we don't want blobstore failure to cause an error in
                // changeset fetching
                .or_else(|_| Ok(()))
                .left_future()
        } else {
            Ok(()).into_future().right_future()
        }
    }

    fn get_changeset_entry(
        &self,
        cs_id: ChangesetId,
    ) -> impl Future<Item = ChangesetEntry, Error = Error> {
        let cache_key = get_cache_key(&self.repo_id, &cs_id);

        cloned!(self.repo_id, self.cache_misses);
        cachelib::get_cached_or_fill(&self.cache_pool, cache_key, move || {
            cache_misses.fetch_add(1, Ordering::Relaxed);
            self.changesets.get(repo_id, cs_id)
        }).and_then(move |maybe_cs| maybe_cs.ok_or_else(|| err_msg(format!("{} not found", cs_id))))
            .and_then({
                let cs_fetcher = self.clone();
                move |cs| {
                    if cs_fetcher.too_many_cache_misses() {
                        cs_fetcher.fill_cache(cs.gen).map(|()| cs).left_future()
                    } else {
                        future::ok(cs).right_future()
                    }
                }
            })
    }
}

impl ChangesetFetcher for CachingChangesetFetcher {
    fn get_generation_number(&self, cs_id: ChangesetId) -> BoxFuture<Generation, Error> {
        self.get_changeset_entry(cs_id.clone())
            .map(|cs| Generation::new(cs.gen))
            .boxify()
    }

    fn get_parents(&self, cs_id: ChangesetId) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.get_changeset_entry(cs_id.clone())
            .map(|cs| cs.parents)
            .boxify()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use async_unit;
    use cachelib::{get_or_create_pool, init_cache_once, LruCacheConfig, LruCachePool};
    use changesets::{serialize_cs_entries, ChangesetEntry, ChangesetInsert};
    use mercurial_types_mocks::repo::REPO_ZERO;
    use mononoke_types::BlobstoreBytes;
    use mononoke_types_mocks::changesetid::{FIVES_CSID, FOURS_CSID, ONES_CSID, THREES_CSID,
                                            TWOS_CSID};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct TestChangesets {
        changesets: Mutex<HashMap<ChangesetId, ChangesetEntry>>,
        get_counter: Arc<AtomicUsize>,
    }

    impl TestChangesets {
        fn new(get_counter: Arc<AtomicUsize>) -> Self {
            Self {
                changesets: Mutex::new(HashMap::new()),
                get_counter,
            }
        }
    }

    impl Changesets for TestChangesets {
        fn add(&self, cs_insert: ChangesetInsert) -> BoxFuture<bool, Error> {
            let mut changesets = self.changesets.lock().unwrap();

            let mut max_gen: u64 = 0;
            for p in cs_insert.parents.iter() {
                match changesets.get(p) {
                    Some(p) => {
                        max_gen = ::std::cmp::max(p.gen, max_gen);
                    }
                    None => {
                        panic!("parent does not exist");
                    }
                }
            }

            if changesets.contains_key(&cs_insert.cs_id) {
                Ok(true).into_future().boxify()
            } else {
                let gen = if cs_insert.parents.is_empty() {
                    0
                } else {
                    max_gen + 1
                };
                changesets.insert(
                    cs_insert.cs_id,
                    ChangesetEntry {
                        repo_id: cs_insert.repo_id,
                        cs_id: cs_insert.cs_id,
                        parents: cs_insert.parents,
                        gen,
                    },
                );
                Ok(false).into_future().boxify()
            }
        }

        fn get(
            &self,
            _repo_id: RepositoryId,
            cs_id: ChangesetId,
        ) -> BoxFuture<Option<ChangesetEntry>, Error> {
            let changesets = self.changesets.lock().unwrap();
            self.get_counter.fetch_add(1, Ordering::Relaxed);
            Ok(changesets.get(&cs_id).cloned()).into_future().boxify()
        }
    }

    #[derive(Debug)]
    struct TestBlobstore {
        blobstore: Mutex<HashMap<String, BlobstoreBytes>>,
        get_counter: Arc<AtomicUsize>,
    }

    impl TestBlobstore {
        fn new(get_counter: Arc<AtomicUsize>) -> Self {
            Self {
                blobstore: Mutex::new(HashMap::new()),
                get_counter,
            }
        }
    }

    impl Blobstore for TestBlobstore {
        fn get(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
            let blobstore = self.blobstore.lock().unwrap();
            self.get_counter.fetch_add(1, Ordering::Relaxed);
            Ok(blobstore.get(&key).cloned()).into_future().boxify()
        }

        fn put(&self, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
            let mut blobstore = self.blobstore.lock().unwrap();
            blobstore.insert(key, value);
            Ok(()).into_future().boxify()
        }
    }

    fn create_stack(changesets: &Changesets) {
        let cs = ChangesetInsert {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
        };
        changesets.add(cs).wait().unwrap();

        let cs = ChangesetInsert {
            repo_id: REPO_ZERO,
            cs_id: TWOS_CSID,
            parents: vec![ONES_CSID],
        };
        changesets.add(cs).wait().unwrap();

        let cs = ChangesetInsert {
            repo_id: REPO_ZERO,
            cs_id: THREES_CSID,
            parents: vec![TWOS_CSID],
        };
        changesets.add(cs).wait().unwrap();

        let cs = ChangesetInsert {
            repo_id: REPO_ZERO,
            cs_id: FOURS_CSID,
            parents: vec![THREES_CSID],
        };
        changesets.add(cs).wait().unwrap();

        let cs = ChangesetInsert {
            repo_id: REPO_ZERO,
            cs_id: FIVES_CSID,
            parents: vec![FOURS_CSID],
        };
        changesets.add(cs).wait().unwrap();
    }

    fn get_cache_pool() -> LruCachePool {
        let config = LruCacheConfig::new(128 * 1024 * 1024);
        init_cache_once(config).unwrap();
        get_or_create_pool("somepool", 4 * 1024 * 1024).unwrap()
    }

    #[test]
    fn test_changeset_fetcher_simple() {
        async_unit::tokio_unit_test(|| {
            let cache_pool = get_cache_pool();
            let cs_get_counter = Arc::new(AtomicUsize::new(0));
            let changesets = TestChangesets::new(cs_get_counter.clone());
            create_stack(&changesets);
            let cs = Arc::new(changesets);

            let blobstore_get_counter = Arc::new(AtomicUsize::new(0));
            let cs_fetcher = CachingChangesetFetcher::new(
                cs,
                REPO_ZERO,
                cache_pool,
                Arc::new(TestBlobstore::new(blobstore_get_counter.clone())),
            );

            cs_fetcher.get_generation_number(ONES_CSID).wait().unwrap();
            assert_eq!(blobstore_get_counter.load(Ordering::Relaxed), 0);
            assert_eq!(cs_get_counter.load(Ordering::Relaxed), 1);
        });
    }

    #[test]
    fn test_changeset_fetcher_no_entry_in_blobstore() {
        async_unit::tokio_unit_test(|| {
            let cache_pool = get_cache_pool();

            let cs_get_counter = Arc::new(AtomicUsize::new(0));
            let changesets = TestChangesets::new(cs_get_counter.clone());
            create_stack(&changesets);
            let cs = Arc::new(changesets);
            let blobstore_get_counter = Arc::new(AtomicUsize::new(0));
            let blobstore = Arc::new(TestBlobstore::new(blobstore_get_counter.clone()));
            let cs_fetcher = CachingChangesetFetcher::new_with_opts(
                cs,
                REPO_ZERO,
                cache_pool,
                blobstore,
                0, /* will try to go to blobstore on every fetch */
                2, /* 0, 2, 4 etc gen numbers  might have a cache entry */
            );

            cs_fetcher.get_generation_number(FIVES_CSID).wait().unwrap();
            assert_eq!(blobstore_get_counter.load(Ordering::Relaxed), 1);
            assert_eq!(cs_get_counter.load(Ordering::Relaxed), 1);
        });
    }

    #[test]
    fn test_changeset_fetcher_entry_in_blobstore() {
        async_unit::tokio_unit_test(|| {
            let cache_pool = get_cache_pool();

            let cs_get_counter = Arc::new(AtomicUsize::new(0));
            let changesets = TestChangesets::new(cs_get_counter.clone());
            create_stack(&changesets);
            let cs = Arc::new(changesets);
            let blobstore_get_counter = Arc::new(AtomicUsize::new(0));
            let blobstore = TestBlobstore::new(blobstore_get_counter.clone());

            // Blob cache entries with gen number 0 up to 4
            blobstore.put(
                "changesetscache_4".to_string(),
                BlobstoreBytes::from_bytes(serialize_cs_entries(vec![
                    cs.get(REPO_ZERO, FIVES_CSID).wait().unwrap().unwrap(),
                    cs.get(REPO_ZERO, FOURS_CSID).wait().unwrap().unwrap(),
                ])),
            );

            cs_get_counter.store(0, Ordering::Relaxed);

            let blobstore = Arc::new(blobstore);
            let cs_fetcher = CachingChangesetFetcher::new_with_opts(
                cs,
                REPO_ZERO,
                cache_pool,
                blobstore,
                0, /* will try to go to blobstore on every fetch */
                2, /* 0, 2, 4 etc gen numbers  might have a cache entry */
            );

            assert_eq!(blobstore_get_counter.load(Ordering::Relaxed), 0);
            cs_fetcher.get_generation_number(FIVES_CSID).wait().unwrap();
            cs_fetcher.get_generation_number(FOURS_CSID).wait().unwrap();
            assert_eq!(blobstore_get_counter.load(Ordering::Relaxed), 1);
            assert_eq!(cs_get_counter.load(Ordering::Relaxed), 1);
        });
    }
}
