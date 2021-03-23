/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::sync::Arc;

use futures::StreamExt;

use crate::newstore::{BoxedWriteStore, WriteResults, WriteStore, WriteStream};

/// A minimal "filter_map" store, which filters writes to the associated `write_store`,
pub struct FilterMapStore<K, V, F> {
    /// The filter_map function used to guard the `write_store`.
    pub filter_map: F,

    /// A `WriteStore` guarded by the filter_map.
    pub write_store: BoxedWriteStore<K, V>,
}

impl<K, V, F> WriteStore<K, V> for FilterMapStore<K, V, F>
where
    K: fmt::Display + fmt::Debug + Send + Sync + 'static,
    V: Send + Sync + 'static,
    F: Fn(V) -> Option<V> + Send + Sync + 'static,
{
    fn write_stream(self: Arc<Self>, values: WriteStream<V>) -> WriteResults<K> {
        self.write_store
            .clone()
            .write_stream(Box::pin(values.filter_map(move |value| {
                let self_ = self.clone();
                async move { (self_.filter_map)(value) }
            })))
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
        newstore::{FetchError, HashMapStore, ReadStore, WriteStore},
    };

    #[test]
    fn test_filtermaps_writes() {
        // The entry that will pass the filter, and be modified
        let entry_key = key("a", "1");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let metadata = Metadata::default();
        let entry = Entry::new(entry_key.clone(), content, metadata.clone());

        // The entry that will fail the filter
        let entry_key2 = key("b", "2");
        let content2 = Bytes::from(&[2, 3, 4, 5][..]);
        let entry2 = Entry::new(entry_key2.clone(), content2, metadata.clone());

        // The modified version of the passing entry
        let content_mod = Bytes::from(&[5, 2, 3, 4][..]);
        let entry_mod = Entry::new(entry_key.clone(), content_mod, metadata.clone());

        let entries = vec![entry, entry2];

        let teststore = Arc::new(HashMapStore::new());

        let filtermapstore = Arc::new(FilterMapStore {
            filter_map: |mut v: Entry| {
                let content = v.content().expect("failed to read content");
                if content[0] == 1 {
                    let mut modified = content.to_vec();
                    modified[0] = 5;
                    Some(Entry::new(
                        v.key().clone(),
                        modified.into(),
                        v.metadata().clone(),
                    ))
                } else {
                    None
                }
            },
            write_store: teststore.clone(),
        });

        // Write test data
        let written: Vec<_> =
            block_on_stream(filtermapstore.write_stream(Box::pin(stream::iter(entries)))).collect();

        assert_eq!(
            written
                .into_iter()
                .map(|r| r.expect("failed to write to test write store"))
                .collect::<Vec<_>>(),
            vec![entry_key.clone()]
        );

        // Read what was written
        let fetched: Vec<_> = block_on_stream(
            teststore.fetch_stream(Box::pin(stream::iter(vec![entry_key, entry_key2.clone()]))),
        )
        .collect();

        let exepcted = vec![Ok(entry_mod), Err(FetchError::not_found(entry_key2))];

        assert_eq!(fetched.into_iter().collect::<Vec<_>>(), exepcted);
    }
}
