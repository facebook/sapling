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

use anyhow::{anyhow, Error};
use async_trait::async_trait;
use futures::{FutureExt, StreamExt};
use tokio::task::spawn_blocking;

use types::Key;

use crate::{
    datastore::{HgIdDataStore, StoreResult},
    indexedlogdatastore::Entry,
    newstore::{FetchStream, KeyStream, ReadStore},
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
            spawn_blocking(move || -> Result<Entry, (Option<Key>, Error)> {
                use StoreResult::*;
                let key = key_;
                let store_key = StoreKey::HgId(key.clone());
                let blob = match self_.0.get(store_key.clone()) {
                    Ok(Found(v)) => v.into(),
                    // TODO: Add type-safe "not found" to new storage traits. We now have two sets
                    // of adapters that support this being downgraded to non-type-safe "not found".
                    Ok(NotFound(_k)) => {
                        return Err((Some(key.clone()), anyhow!("key not found")));
                    }
                    Err(e) => {
                        return Err((Some(key.clone()), e));
                    }
                };
                let meta = match self_.0.get_meta(store_key) {
                    Ok(Found(v)) => v,
                    Ok(NotFound(_k)) => {
                        return Err((Some(key.clone()), anyhow!("key not found")));
                    }
                    Err(e) => {
                        return Err((Some(key.clone()), e));
                    }
                };

                Ok(Entry::new(key, blob, meta))
            })
            .map(move |spawn_res| {
                match spawn_res {
                    Ok(Ok(entry)) => Ok(entry),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err((Some(key), e.into())),
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
}
