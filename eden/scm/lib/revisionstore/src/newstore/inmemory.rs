/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, fmt, hash::Hash, sync::Arc};

use futures::{lock::Mutex, StreamExt};

use crate::newstore::{
    FetchError, FetchStream, KeyStream, ReadStore, WriteResults, WriteStore, WriteStream,
};

pub struct HashMapStore<K, V> {
    store: Mutex<HashMap<K, V>>,
}

impl<K, V> HashMapStore<K, V> {
    pub fn new() -> Self {
        HashMapStore {
            store: Mutex::new(HashMap::new()),
        }
    }
}

/// A value type which can return it's key.
///
/// Currently only used for testing, but we'll likely want to have a `StoreValue` trait
/// for other purposes in the future. We'll also probably want to lift the associated type
/// to a generic to support values which can be keyed with multiple key types.
pub trait KeyedValue {
    type Key;

    fn key(&self) -> Self::Key;
}

impl<K, V> ReadStore<K, V> for HashMapStore<K, V>
where
    K: fmt::Display + fmt::Debug + std::cmp::Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, V> {
        Box::pin(keys.then(move |key| {
            let self_ = self.clone();
            async move {
                self_
                    .store
                    .lock()
                    .await
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| FetchError::not_found(key))
            }
        }))
    }
}

impl<K, V> WriteStore<K, V> for HashMapStore<K, V>
where
    K: Clone + fmt::Display + fmt::Debug + std::cmp::Eq + Hash + Send + Sync + 'static,
    V: KeyedValue<Key = K> + Send + Sync + 'static,
{
    fn write_stream(self: Arc<Self>, values: WriteStream<V>) -> WriteResults<K> {
        Box::pin(values.then(move |value| {
            let self_ = self.clone();
            async move {
                let key = value.key();
                self_.store.lock().await.insert(key.clone(), value);
                Ok(key)
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::stream;
    use minibytes::Bytes;

    use async_runtime::stream_to_iter as block_on_stream;
    use types::testutil::*;

    use crate::{
        datastore::Metadata,
        indexedlogdatastore::Entry,
        newstore::{ReadStore, WriteStore},
    };

    #[test]
    fn test_write_read() {
        let entry_key = key("a", "1");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let metadata = Metadata::default();
        let entry = Entry::new(entry_key.clone(), content, metadata.clone());
        let entries = vec![entry];

        let teststore = Arc::new(HashMapStore::new());

        // Write test data
        let written: Vec<_> = block_on_stream(
            teststore
                .clone()
                .write_stream(Box::pin(stream::iter(entries.clone()))),
        )
        .collect();

        assert_eq!(
            written
                .into_iter()
                .map(|r| r.expect("failed to write to test write store"))
                .collect::<Vec<_>>(),
            vec![entry_key.clone()]
        );

        // Read, also using legacy wrapper
        let fetched: Vec<_> =
            block_on_stream(teststore.fetch_stream(Box::pin(stream::iter(vec![entry_key]))))
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
