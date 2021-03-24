/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures_batch::ChunksTimeoutStreamExt;

use edenapi::EdenApi;
use edenapi_types::{FileEntry, TreeAttributes, TreeEntry};
use types::Key;

use crate::{
    localstore::ExtStoredPolicy,
    newstore::{fetch_error, FetchError, FetchStream, KeyStream, ReadStore},
};

// TODO(meyer): These should be configurable
// EdenApi's API is batch-based and async, and it will split a large batch into multiple requests to send in parallel
// but it won't join separate batches into larger ones. Because the input stream may not terminate in a timely fashion,
// we group the stream into batches with a timeout so that EdenApi will actually be sent batches, rather than constructing
// a batch of one for each item in the stream. This is worth investigating in the future, though - we could be sending
// "batches of one" to EdenApi, or we could change the EdenApi client to batch across requests, not just within them.
// I believe Arun has determined that even with HTTP2, some level of batching within requests is advantageous instead
// of individually streaming a separate request for each key, but it's still worth making sure we're doing the rgiht thing.
// We might also want to just grab all ready items from the stream in a batch, with no timeout, if the cost of small batches
// is smaller than the cost of the timeout waiting to collect larger ones.
const BATCH_SIZE: usize = 100;
const BATCH_TIMEOUT: Duration = Duration::from_millis(100);

pub struct EdenApiAdapter<C> {
    pub client: C,
    pub repo: String,
    pub extstored_policy: ExtStoredPolicy,
}

impl<C> ReadStore<Key, TreeEntry> for EdenApiAdapter<C>
where
    C: EdenApi,
{
    fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<Key, TreeEntry> {
        Box::pin(
            keys.chunks_timeout(BATCH_SIZE, BATCH_TIMEOUT)
                .then(move |keys| {
                    let self_ = self.clone();
                    async move {
                        self_
                            .client
                            .trees(self_.repo.clone(), keys, Some(TreeAttributes::all()), None)
                            .await
                            .map_or_else(fetch_error, |s| {
                                Box::pin(s.entries.map(|v| match v {
                                    Ok(Ok(v)) => Ok(v),
                                    // TODO: Separate out NotFound errors from EdenApi
                                    // TODO: We could eliminate this redundant key clone with a trait, I think.
                                    Ok(Err(e)) => Err(FetchError::maybe_with_key(e.key.clone(), e)),
                                    // TODO: What should happen when an entire batch fails?
                                    Err(e) => Err(FetchError::from(e)),
                                })) as FetchStream<Key, TreeEntry>
                            })
                    }
                })
                .flatten(),
        )
    }
}

impl<C> ReadStore<Key, FileEntry> for EdenApiAdapter<C>
where
    C: EdenApi,
{
    fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<Key, FileEntry> {
        Box::pin(
            keys.chunks_timeout(BATCH_SIZE, BATCH_TIMEOUT)
                .then(move |keys| {
                    let self_ = self.clone();
                    async move {
                        self_
                            .client
                            .files(self_.repo.clone(), keys, None)
                            .await
                            .map_or_else(fetch_error, {
                                let self_ = self_.clone();
                                move |fetch| {
                                    // TODO: Add per-item errors to EdenApi `files`
                                    Box::pin(fetch.entries.map(move |res| {
                                        res.map_err(FetchError::from).and_then(|entry| {
                                            if self_.extstored_policy == ExtStoredPolicy::Ignore
                                                && entry.metadata().is_lfs()
                                            {
                                                Err(FetchError::not_found(entry.key().clone()))
                                            } else {
                                                Ok(entry)
                                            }
                                        })
                                    }))
                                        as
                                        FetchStream<Key, FileEntry>
                                }
                            })
                    }
                })
                .flatten(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use futures::stream;
    use maplit::hashmap;

    use async_runtime::stream_to_iter as block_on_stream;
    use minibytes::Bytes;
    use types::{testutil::*, Parents};

    use crate::{
        datastore::Metadata,
        edenapi::{EdenApiRemoteStore, File},
        localstore::ExtStoredPolicy,
        newstore::FetchError,
        testutil::*,
    };

    #[test]
    fn test_files_extstore_use() -> Result<(), ()> {
        // Set up mocked EdenAPI file and tree stores.
        let lfs_metadata = Metadata {
            size: Some(4),
            flags: Some(Metadata::LFS_FLAG),
        };
        let nonlfs_metadata = Metadata {
            size: Some(4),
            flags: None,
        };

        let lfs_key = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let nonlfs_key = key("b", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");

        let lfs_bytes = Bytes::from("1234");
        let nonlfs_bytes = Bytes::from("2345");

        let files = hashmap! {
            lfs_key.clone() => (lfs_bytes.clone(), lfs_metadata.flags),
            nonlfs_key.clone() => (nonlfs_bytes.clone(), nonlfs_metadata.flags)
        };
        let trees = HashMap::new();

        let client = FakeEdenApi::new()
            .files_with_flags(files)
            .trees(trees)
            .into_arc();
        let remote_files = EdenApiRemoteStore::<File>::new("repo", client, None);

        let files_adapter = Arc::new(remote_files.get_newstore_adapter(ExtStoredPolicy::Use));

        let fetched: Vec<_> =
            block_on_stream(files_adapter.fetch_stream(Box::pin(stream::iter(vec![
                lfs_key.clone(),
                nonlfs_key.clone(),
            ]))))
            .collect();

        let lfs_entry = FileEntry::new(
            lfs_key,
            lfs_bytes.to_vec().into(),
            Parents::default(),
            lfs_metadata,
        );
        let nonlfs_entry = FileEntry::new(
            nonlfs_key,
            nonlfs_bytes.to_vec().into(),
            Parents::default(),
            nonlfs_metadata,
        );

        let exepcted = vec![Ok(lfs_entry), Ok(nonlfs_entry)];

        assert_eq!(fetched.into_iter().collect::<Vec<_>>(), exepcted);

        Ok(())
    }

    #[test]
    fn test_files_extstore_ignore() -> Result<(), ()> {
        // Set up mocked EdenAPI file and tree stores.
        let lfs_metadata = Metadata {
            size: Some(4),
            flags: Some(Metadata::LFS_FLAG),
        };
        let nonlfs_metadata = Metadata {
            size: Some(4),
            flags: None,
        };

        let lfs_key = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let nonlfs_key = key("b", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");

        let lfs_bytes = Bytes::from("1234");
        let nonlfs_bytes = Bytes::from("2345");

        let files = hashmap! {
            lfs_key.clone() => (lfs_bytes.clone(), lfs_metadata.flags),
            nonlfs_key.clone() => (nonlfs_bytes.clone(), nonlfs_metadata.flags)
        };

        let trees = HashMap::new();

        let client = FakeEdenApi::new()
            .files_with_flags(files)
            .trees(trees)
            .into_arc();
        let remote_files = EdenApiRemoteStore::<File>::new("repo", client, None);

        let files_adapter = Arc::new(remote_files.get_newstore_adapter(ExtStoredPolicy::Ignore));

        let fetched: Vec<_> =
            block_on_stream(files_adapter.fetch_stream(Box::pin(stream::iter(vec![
                lfs_key.clone(),
                nonlfs_key.clone(),
            ]))))
            .collect();

        let _lfs_entry = FileEntry::new(
            lfs_key.clone(),
            lfs_bytes.to_vec().into(),
            Parents::default(),
            lfs_metadata,
        );
        let nonlfs_entry = FileEntry::new(
            nonlfs_key,
            nonlfs_bytes.to_vec().into(),
            Parents::default(),
            nonlfs_metadata,
        );

        let exepcted = vec![Err(FetchError::not_found(lfs_key)), Ok(nonlfs_entry)];

        assert_eq!(fetched.into_iter().collect::<Vec<_>>(), exepcted);

        Ok(())
    }
}
