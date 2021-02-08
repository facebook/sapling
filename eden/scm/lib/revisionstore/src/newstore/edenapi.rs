/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use async_trait::async_trait;
use futures::StreamExt;
use futures_batch::ChunksTimeoutStreamExt;

use edenapi::EdenApi;
use edenapi_types::{TreeAttributes, TreeEntry};
use types::Key;

use crate::newstore::{fetch_error, FetchStream, KeyStream, ReadStore};

// TODO: These should be configurable
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
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<TreeEntry> {
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
                                    Ok(Err(e)) => Err(Error::new(e)),
                                    Err(e) => Err(Error::new(e)),
                                })) as FetchStream<TreeEntry>
                            })
                    }
                })
                .flatten(),
        )
    }
}
