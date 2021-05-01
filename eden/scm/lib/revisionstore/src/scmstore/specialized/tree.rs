/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::sync::Arc;

use anyhow::Result;

use types::Key;

use crate::{
    datastore::{HgIdDataStore, RemoteDataStore},
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore},
    ContentStore, EdenApiTreeStore, MemcacheStore, StoreKey, StoreResult,
};

pub struct TreeStore {
    pub indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    pub indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    pub cache_to_local_cache: bool,

    pub memcache: Option<Arc<MemcacheStore>>,
    pub cache_to_memcache: bool,

    pub edenapi: Option<Arc<EdenApiTreeStore>>,
    pub contentstore: Option<Arc<ContentStore>>,
}

impl TreeStore {
    fn fetch_batch(&self, reqs: Vec<Key>) -> Result<Vec<Entry>> {
        // TODO(meyer): Need to standardize all these APIs to accept batches in the same way,
        // ideally supporting an iterator of references to keys (since Key is not Copy) or something
        // general like that (StoreKey, etc)

        // TODO(meyer): Can improve this considerably, but it's just to hack everything out
        // (redundant key clones, can combine the hash sets and maps, can remove the collecting incomplete
        // into Vec repeatedly). Also obviously the error handling is shit, a bunch of stuff fails the batch.

        let mut complete = HashMap::<Key, Entry>::new();
        let mut write_to_local_cache = HashSet::new();
        let mut write_to_memcache = HashSet::new();
        let mut incomplete: HashSet<_> = reqs.into_iter().collect();

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            let pending: Vec<_> = incomplete.iter().cloned().collect();
            let indexedlog_cache = indexedlog_cache.read_lock();
            for key in pending.into_iter() {
                if let Some(entry) = indexedlog_cache.get_entry(key)? {
                    incomplete.remove(entry.key());
                    complete.insert(entry.key().clone(), entry);
                }
            }
        }

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            let pending: Vec<_> = incomplete.iter().cloned().collect();
            let indexedlog_local = indexedlog_local.read_lock();
            for key in pending.into_iter() {
                if let Some(entry) = indexedlog_local.get_entry(key)? {
                    incomplete.remove(entry.key());
                    complete.insert(entry.key().clone(), entry);
                }
            }
        }

        if let Some(ref memcache) = self.memcache {
            let pending: Vec<_> = incomplete.iter().cloned().collect();

            for entry in memcache.get_data_iter(&pending)? {
                let entry: Entry = entry?.into();
                incomplete.remove(entry.key());
                if self.indexedlog_cache.is_some() && self.cache_to_local_cache {
                    write_to_local_cache.insert(entry.key().clone());
                }
                complete.insert(entry.key().clone(), entry);
            }
        }

        if let Some(ref edenapi) = self.edenapi {
            let pending: Vec<_> = incomplete.iter().cloned().collect();

            for entry in edenapi.trees_blocking(pending, None, None)?.entries {
                // TODO(meyer): Should probably remove the From impls and add TryFrom instead
                // TODO(meyer): Again, handle errors better. This will turn EdenApi NotFound into failing
                // the entire batch
                let entry: Entry = entry?.into();
                incomplete.remove(entry.key());
                if self.indexedlog_cache.is_some() && self.cache_to_local_cache {
                    write_to_local_cache.insert(entry.key().clone());
                }
                if self.memcache.is_some() && self.cache_to_memcache {
                    write_to_memcache.insert(entry.key().clone());
                }
                complete.insert(entry.key().clone(), entry);
            }
        }

        if let Some(ref contentstore) = self.contentstore {
            let pending: Vec<_> = incomplete.iter().cloned().map(StoreKey::HgId).collect();
            contentstore.prefetch(&pending)?;

            let pending = pending.into_iter().map(|key| match key {
                StoreKey::HgId(key) => key,
                // Safe because we constructed pending with only StoreKey::HgId above
                // we're just re-using the already allocated paths in the keys
                _ => unreachable!("unexpected non-HgId StoreKey"),
            });
            for key in pending {
                let store_key = StoreKey::HgId(key.clone());
                let blob = match contentstore.get(store_key.clone())? {
                    StoreResult::Found(v) => Some(v),
                    StoreResult::NotFound(_k) => None,
                };
                let meta = match contentstore.get_meta(store_key)? {
                    StoreResult::Found(v) => Some(v),
                    StoreResult::NotFound(_k) => None,
                };

                if let (Some(blob), Some(meta)) = (blob, meta) {
                    incomplete.remove(&key);
                    // We don't write to local indexedlog or memcache for contentstore fallbacks because
                    // contentstore handles that internally.
                    let entry = Entry::new(key, blob.into(), meta);
                    complete.insert(entry.key().clone(), entry);
                }
            }
        }

        // TODO(meyer): We can do this in the background if we actually want to make this implementation perform well.
        // TODO(meyer): We shouldn't fail the batch on write failures here.
        if self.cache_to_local_cache {
            if let Some(ref indexedlog_cache) = self.indexedlog_cache {
                let mut indexedlog_cache = indexedlog_cache.write_lock();
                for key in write_to_local_cache.iter() {
                    indexedlog_cache.put_entry(complete[key].clone())?
                }
            }
        }
        if self.cache_to_memcache {
            if let Some(ref memcache) = self.memcache {
                for key in write_to_memcache.iter() {
                    memcache.add_mcdata(complete[key].clone().try_into()?);
                }
            }
        }

        // TODO(meyer): Report incomplete / not found, handle errors better instead of just always failing the batch, etc

        Ok(complete.drain().map(|(_k, v)| v).collect())
    }
}
