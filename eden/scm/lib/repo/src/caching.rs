/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use lru_cache::LruCache;
use parking_lot::Mutex;
use storemodel::BoxIterator;
use storemodel::Bytes;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use types::fetch_mode::FetchMode;
use types::HgId;
use types::Key;
use types::RepoPath;

pub struct CachingKeyStore {
    store: Arc<dyn KeyStore>,
    cache: Arc<Mutex<LruCache<HgId, Bytes>>>,
}

impl CachingKeyStore {
    pub fn new(store: Arc<dyn KeyStore>, size: usize) -> Self {
        Self {
            store,
            cache: Arc::new(Mutex::new(LruCache::new(size))),
        }
    }

    // Fetch a single item from cache.
    pub(crate) fn cached_single(&self, id: &HgId) -> Option<Bytes> {
        self.cache.lock().get_mut(id).cloned()
    }

    // Fetch multiple items from cache, returning (misses, hits).
    pub(crate) fn cached_multi(&self, mut keys: Vec<Key>) -> (Vec<Key>, Vec<(Key, Bytes)>) {
        let mut cache = self.cache.lock();

        let mut found = Vec::new();
        keys.retain(|key| {
            if let Some(data) = cache.get_mut(&key.hgid) {
                found.push((key.clone(), data.clone()));
                false
            } else {
                true
            }
        });

        (keys, found)
    }

    /// Insert a (key, value) pair into the cache.
    /// Note: this does not insert the value into the underlying store
    pub(crate) fn cache_with_key(&self, key: HgId, data: Bytes) -> Result<()> {
        self.cache.lock().insert(key, data.clone());
        Ok(())
    }
}

impl KeyStore for CachingKeyStore {
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> Result<BoxIterator<Result<(Key, Bytes)>>> {
        let (keys, cached) = self.cached_multi(keys);

        let uncached = CachingIter {
            iter: self.store.get_content_iter(keys, fetch_mode)?,
            cache: self.cache.clone(),
        };

        Ok(Box::new(uncached.chain(cached.into_iter().map(Ok))))
    }

    fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> Result<Option<Bytes>> {
        if let Some(cached) = self.cached_single(&hgid) {
            Ok(Some(cached))
        } else {
            match self.store.get_local_content(path, hgid) {
                Ok(Some(data)) => {
                    self.cache.lock().insert(hgid, data.clone());
                    Ok(Some(data))
                }
                r => r,
            }
        }
    }

    fn get_content(&self, path: &RepoPath, hgid: HgId, fetch_mode: FetchMode) -> Result<Bytes> {
        if let Some(cached) = self.cached_single(&hgid) {
            Ok(cached)
        } else {
            match self.store.get_content(path, hgid, fetch_mode) {
                Ok(data) => {
                    self.cache.lock().insert(hgid, data.clone());
                    Ok(data)
                }
                r => r,
            }
        }
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        // Intercept prefetch() so we can prime our cache. This is what manifest-tree
        // operations like bfs_iter and diff use when walking trees.
        self.get_content_iter(keys, FetchMode::AllowRemote)?
            .for_each(|_| ());
        Ok(())
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> Result<HgId> {
        self.store.insert_data(opts, path, data)
    }

    fn flush(&self) -> Result<()> {
        self.store.flush()
    }

    fn refresh(&self) -> Result<()> {
        self.store.refresh()
    }

    fn format(&self) -> SerializationFormat {
        self.store.format()
    }

    fn statistics(&self) -> Vec<(String, usize)> {
        self.store.statistics()
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        self.store.clone_key_store()
    }
}

// An Iterator that lazily populates tree cache during iteration.
struct CachingIter {
    iter: BoxIterator<Result<(Key, Bytes)>>,
    cache: Arc<Mutex<LruCache<HgId, Bytes>>>,
}

impl Iterator for CachingIter {
    type Item = Result<(Key, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(item) => {
                if let Ok((key, data)) = &item {
                    self.cache.lock().insert(key.hgid, data.clone());
                }
                Some(item)
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod test {
    use manifest_tree::testutil::TestStore;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaChaRng;
    use types::RepoPathBuf;

    use super::*;

    #[test]
    fn test_key_cache() -> Result<()> {
        let inner_store = Arc::new(TestStore::new());

        let caching_store = CachingKeyStore::new(inner_store.clone(), 5);

        let val1 = RepoPathBuf::from_string("val1".to_string())?;
        let val2 = RepoPathBuf::from_string("val2".to_string())?;

        let val1_id = caching_store.insert_data(Default::default(), &val1, b"val1")?;
        let val2_id = caching_store.insert_data(Default::default(), &val2, b"val2")?;
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let val3_id = HgId::random(&mut rng);

        assert_eq!(inner_store.key_fetch_count(), 0);

        assert_eq!(
            caching_store.get_content(&val1, val1_id, FetchMode::AllowRemote)?,
            b"val1"
        );
        assert_eq!(inner_store.key_fetch_count(), 1);

        // Fetch again - make sure we cached it.
        assert_eq!(
            caching_store.get_content(&val1, val1_id, FetchMode::AllowRemote)?,
            b"val1"
        );
        assert_eq!(inner_store.key_fetch_count(), 1);

        // Fetch both via iterator.
        let key1 = Key::new(val1.clone(), val1_id);
        let key2 = Key::new(val2.clone(), val2_id);
        assert_eq!(
            caching_store
                .get_content_iter(vec![key1.clone(), key2.clone()], FetchMode::AllowRemote)?
                .collect::<Result<Vec<_>>>()?,
            vec![
                (key2.clone(), b"val2".as_ref().into()),
                (key1.clone(), b"val1".as_ref().into()),
            ]
        );
        // Should only have done 1 additional fetch for val2.
        assert_eq!(inner_store.key_fetch_count(), 2);

        assert_eq!(
            caching_store
                .get_content_iter(vec![key1.clone(), key2.clone()], FetchMode::AllowRemote)?
                .collect::<Result<Vec<_>>>()?,
            vec![
                (key1.clone(), b"val1".as_ref().into()),
                (key2.clone(), b"val2".as_ref().into()),
            ]
        );

        caching_store.prefetch(vec![key1.clone(), key2.clone()])?;

        assert_eq!(inner_store.key_fetch_count(), 2);

        // Ensure only the cache is modified; not the underlying store
        let insert_count = inner_store.insert_count();
        caching_store.cache_with_key(val3_id.clone(), b"val3".as_ref().into())?;
        assert_eq!(insert_count, inner_store.insert_count());
        let cached_value = caching_store
            .cached_single(&val3_id)
            .expect("value to be cached");
        assert_eq!(cached_value, b"val3");

        Ok(())
    }
}
