/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use futures_batch::ChunksTimeoutStreamExt;

use edenapi::EdenApi;
use edenapi_types::{FileEntry, TreeAttributes, TreeEntry};
use types::Key;

use crate::newstore::{fetch_error, FetchError, FetchStream, KeyStream, ReadStore};

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
}

#[async_trait]
impl<C> ReadStore<Key, TreeEntry> for EdenApiAdapter<C>
where
    C: EdenApi,
{
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<Key, TreeEntry> {
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

#[async_trait]
impl<C> ReadStore<Key, FileEntry> for EdenApiAdapter<C>
where
    C: EdenApi,
{
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<Key, FileEntry> {
        Box::pin(
            keys.chunks_timeout(BATCH_SIZE, BATCH_TIMEOUT)
                .then(move |keys| {
                    let self_ = self.clone();
                    async move {
                        self_
                            .client
                            .files(self_.repo.clone(), keys, None)
                            .await
                            .map_or_else(fetch_error, |s| {
                                // TODO: Add per-item errors to EdenApi `files`
                                Box::pin(s.entries.map(|v| v.map_err(FetchError::from)))
                                    as FetchStream<Key, FileEntry>
                            })
                    }
                })
                .flatten(),
        )
    }
}
