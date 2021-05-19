/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// TODO(meyer): Remove this
#![allow(dead_code)]
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};

use minibytes::Bytes;
use types::Key;

use crate::{
    datastore::{HgIdDataStore, RemoteDataStore},
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore},
    ContentDataStore, ContentMetadata, ContentStore, Delta, EdenApiTreeStore,
    HgIdMutableDeltaStore, LocalStore, MemcacheStore, Metadata, StoreKey, StoreResult,
};

pub struct TreeStore {
    /// The "local" indexedlog store. Stores content that is created locally.
    pub indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,

    /// The "cache" indexedlog store (previously called "shared"). Stores content downloaded from
    /// a remote store.
    pub indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,

    /// If cache_to_local_cache is true, data found by falling back to a remote store
    /// will the written to indexedlog_cache.
    pub cache_to_local_cache: bool,

    /// If provided, memcache will be checked before other remote stores
    pub memcache: Option<Arc<MemcacheStore>>,

    /// If cache_to_memcache is true, data found by falling back to another remote store
    // will be written to memcache.
    pub cache_to_memcache: bool,

    /// An EdenApi Client, EdenApiTreeStore provides the tree-specific subset of EdenApi functionality
    /// used by TreeStore.
    pub edenapi: Option<Arc<EdenApiTreeStore>>,

    /// Hook into the legacy storage architecture, if we fall back to this and succeed, we
    /// should alert / log something, as this should never happen if TreeStore is implemented
    /// correctly.
    pub contentstore: Option<Arc<ContentStore>>,
}

impl Drop for TreeStore {
    fn drop(&mut self) {
        // TODO(meyer): Should we just add a Drop impl for IndexedLogHgIdDataStore instead?
        // It'll be called automatically when the last Arc holding a reference to it is dropped.
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            // TODO(meyer): Drop can't fail, so we ignore errors here. We should probably attempt to log them instead.
            let _ = indexedlog_local.flush_log();
        }
        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            let _ = indexedlog_cache.flush_log();
        }
    }
}

pub struct TreeStoreFetch {
    pub complete: Vec<Entry>,
    pub incomplete: Vec<Key>,
}

impl TreeStore {
    fn fetch_batch(&self, reqs: impl Iterator<Item = Key>) -> Result<TreeStoreFetch> {
        let mut complete = HashMap::<Key, Entry>::new();
        let mut write_to_local_cache = HashSet::new();
        let mut write_to_memcache = HashSet::new();
        let mut incomplete: HashSet<_> = reqs.collect();

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
        Ok(TreeStoreFetch {
            complete: complete.drain().map(|(_k, v)| v).collect(),
            incomplete: incomplete.drain().collect(),
        })
    }

    fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            let mut indexedlog_local = indexedlog_local.write_lock();
            for (key, bytes, meta) in entries {
                indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;
            }
        }
        Ok(())
    }
}

impl HgIdDataStore for TreeStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(
            match self
                .fetch_batch(std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key))?
                .complete
                .pop()
            {
                Some(mut entry) => StoreResult::Found(entry.content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        Ok(
            match self
                .fetch_batch(std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key))?
                .complete
                .pop()
            {
                Some(e) => StoreResult::Found(e.metadata().clone()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        // AFAIK refresh only matters for DataPack / PackStore
        Ok(())
    }
}

impl RemoteDataStore for TreeStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(self
            .fetch_batch(keys.iter().cloned().filter_map(StoreKey::maybe_into_key))?
            .incomplete
            .into_iter()
            .map(StoreKey::HgId)
            .collect())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl LocalStore for TreeStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let mut missing: Vec<_> = keys.to_vec();

        missing = if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            let indexedlog_cache = indexedlog_cache.read_lock();
            missing
                .into_iter()
                .filter(
                    |sk| match sk.maybe_as_key().map(|k| indexedlog_cache.get_raw_entry(k)) {
                        Some(Ok(Some(_))) => false,
                        None | Some(Err(_)) | Some(Ok(None)) => true,
                    },
                )
                .collect()
        } else {
            missing
        };

        missing = if let Some(ref indexedlog_local) = self.indexedlog_local {
            let indexedlog_local = indexedlog_local.read_lock();
            missing
                .into_iter()
                .filter(
                    |sk| match sk.maybe_as_key().map(|k| indexedlog_local.get_raw_entry(k)) {
                        Some(Ok(Some(_))) => false,
                        None | Some(Err(_)) | Some(Ok(None)) => true,
                    },
                )
                .collect()
        } else {
            missing
        };

        Ok(missing)
    }
}

impl HgIdMutableDeltaStore for TreeStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        if let Delta {
            data,
            base: None,
            key,
        } = delta.clone()
        {
            self.write_batch(std::iter::once((key, data, metadata.clone())))
        } else {
            bail!("Deltas with non-None base are not supported")
        }
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            indexedlog_local.flush_log()?;
        }
        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            indexedlog_cache.flush_log()?;
        }
        Ok(None)
    }
}

// TODO(meyer): Content addressing not supported at all for trees. I could look for HgIds present here and fetch with
// that if available, but I feel like there's probably something wrong if this is called for trees. Do we need to implement
// this at all for trees?
impl ContentDataStore for TreeStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        Ok(StoreResult::NotFound(key))
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        Ok(StoreResult::NotFound(key))
    }
}
