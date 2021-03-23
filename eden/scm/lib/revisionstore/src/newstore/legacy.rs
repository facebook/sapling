/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Simple shims to adapt the legacy storage API to the new storage API.
//! These adapters will be very slow, and don't benefit from the new storage
//! API's async support or (currently) even the old storage API's batch
//! prefetching.

use std::sync::Arc;

use async_trait::async_trait;
use futures::{FutureExt, StreamExt};
use tokio::task::spawn_blocking;

use types::Key;

use crate::{
    datastore::{Delta, HgIdDataStore, HgIdMutableDeltaStore, StoreResult},
    indexedlogdatastore::Entry,
    newstore::{
        FetchError, FetchStream, KeyStream, ReadStore, WriteError, WriteResults, WriteStore,
        WriteStream,
    },
    types::StoreKey,
};

pub struct LegacyDatastore<T>(pub T);

#[async_trait]
impl<T> ReadStore<Key, Entry> for LegacyDatastore<T>
where
    T: HgIdDataStore + 'static,
{
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<Key, Entry> {
        Box::pin(keys.then(move |key| {
            let self_ = self.clone();
            let key_ = key.clone();
            spawn_blocking(move || {
                use StoreResult::*;
                let key = key_;
                let store_key = StoreKey::HgId(key.clone());
                let blob = match self_.0.get(store_key.clone()) {
                    Ok(Found(v)) => Ok(v.into()),
                    Ok(NotFound(_k)) => Err(FetchError::not_found(key.clone())),
                    Err(e) => Err(FetchError::with_key(key.clone(), e)),
                }?;
                let meta = match self_.0.get_meta(store_key) {
                    Ok(Found(v)) => Ok(v),
                    Ok(NotFound(_k)) => Err(FetchError::not_found(key.clone())),
                    Err(e) => Err(FetchError::with_key(key.clone(), e)),
                }?;

                Ok(Entry::new(key, blob, meta))
            })
            .map(move |spawn_res| {
                match spawn_res {
                    Ok(Ok(entry)) => Ok(entry),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(FetchError::with_key(key, e)),
                }
            })
        }))
    }
}

#[async_trait]
impl<T> WriteStore<Key, Entry> for LegacyDatastore<T>
where
    T: HgIdMutableDeltaStore + 'static,
{
    async fn write_stream(self: Arc<Self>, values: WriteStream<Entry>) -> WriteResults<Key> {
        Box::pin(values.then(move |mut value| {
            let self_ = self.clone();
            let key = value.key().clone();
            spawn_blocking(move || {
                let key = value.key().clone();
                let content = match value.content() {
                    Ok(c) => c,
                    Err(e) => {
                        return Err(WriteError::with_key(key, e));
                    }
                };
                let delta = Delta {
                    data: content,
                    base: None,
                    key: key.clone(),
                };
                match self_.0.add(&delta, value.metadata()) {
                    Ok(()) => Ok(key),
                    Err(e) => Err(WriteError::with_key(key, e)),
                }
            })
            .map(move |spawn_res| {
                match spawn_res {
                    Ok(Ok(entry)) => Ok(entry),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(WriteError::with_key(key, e)),
                }
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::stream;
    use minibytes::Bytes;
    use tempfile::TempDir;

    use async_runtime::{block_on_future as block_on, stream_to_iter as block_on_stream};
    use configparser::config::ConfigSet;
    use types::testutil::*;

    use crate::{
        datastore::{Delta, HgIdMutableDeltaStore, Metadata},
        indexedlogdatastore::{IndexedLogDataStoreType, IndexedLogHgIdDataStore},
        localstore::ExtStoredPolicy,
        newstore::ReadStore,
    };

    #[test]
    fn test_legacy_shim_read() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let entry_key = key("a", "1");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let metadata = Metadata::default();
        let entry = Entry::new(entry_key.clone(), content.clone(), metadata.clone());

        // Write using old API
        let delta = Delta {
            data: content,
            base: None,
            key: entry_key.clone(),
        };

        log.add(&delta, &metadata).unwrap();
        log.flush().unwrap();

        // Wrap to direct new API to call into old API
        let legacy = LegacyDatastore(log);
        let log = Arc::new(legacy);

        let entries = vec![entry];

        let fetched: Vec<_> = block_on_stream(block_on(
            log.fetch_stream(Box::pin(stream::iter(vec![entry_key]))),
        ))
        .collect();

        assert_eq!(
            fetched
                .into_iter()
                .map(|r| r.expect("failed to fetch from test read store"))
                .collect::<Vec<_>>(),
            entries
        );
    }

    #[test]
    fn test_legacy_shim_write() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let entry_key = key("a", "1");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let metadata = Metadata::default();
        let entry = Entry::new(entry_key.clone(), content, metadata.clone());

        // Wrap to direct new API to call into old API
        let legacy = LegacyDatastore(log);
        let log = Arc::new(legacy);

        let entries = vec![entry];

        // Write test data
        let written: Vec<_> = block_on_stream(block_on(
            log.clone()
                .write_stream(Box::pin(stream::iter(entries.clone()))),
        ))
        .collect();

        assert_eq!(
            written
                .into_iter()
                .map(|r| r.expect("failed to write to test write store"))
                .collect::<Vec<_>>(),
            vec![entry_key.clone()]
        );

        // TODO: Add "flush" support to WriteStore trait
        log.0.flush().unwrap();

        // Read, also using legacy wrapper
        let fetched: Vec<_> = block_on_stream(block_on(
            log.fetch_stream(Box::pin(stream::iter(vec![entry_key]))),
        ))
        .collect();

        assert_eq!(
            fetched
                .into_iter()
                .map(|r| r.expect("failed to fetch from test read store"))
                .collect::<Vec<_>>(),
            entries
        );
    }
}
