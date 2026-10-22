/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use edenapi_types::StreamingChangelogResponse;
use futures::prelude::*;
use parking_lot::Mutex;

use super::RetryableStreamRequest;
use crate::client::Client;
use crate::errors::SaplingRemoteApiError;
use crate::response::Response;

pub(crate) struct RetryableStreamingClone {
    tag: Option<String>,
    received: Arc<Mutex<Prefix>>,
}

#[derive(Clone, Default)]
struct Prefix {
    entries: usize,
    hasher: blake3::Hasher,
}

impl Prefix {
    fn push(&mut self, entry: &StreamingChangelogResponse) -> Result<(), SaplingRemoteApiError> {
        serde_cbor::to_writer(&mut self.hasher, entry)
            .context("serializing streaming clone response for retry verification")?;
        self.entries += 1;
        Ok(())
    }
}

impl RetryableStreamingClone {
    pub(crate) fn new(tag: Option<String>) -> Self {
        Self {
            tag,
            received: Default::default(),
        }
    }
}

#[async_trait]
impl RetryableStreamRequest for RetryableStreamingClone {
    type Item = StreamingChangelogResponse;

    async fn perform(&self, client: Client) -> Result<Response<Self::Item>, SaplingRemoteApiError> {
        let Response { mut entries, stats } =
            client.streaming_clone_attempt(self.tag.clone()).await?;

        // Clone responses are ordered. Verify the prefix before resuming: the
        // server's selected snapshot or chunk boundaries can change between attempts.
        let received = self.received.lock().clone();
        let mut replayed = Prefix::default();
        for _ in 0..received.entries {
            let entry = entries.try_next().await?.ok_or_else(|| {
                SaplingRemoteApiError::IncompleteResponse(
                    "Streaming clone retry ended before its previous prefix".into(),
                )
            })?;
            replayed.push(&entry)?;
        }
        if replayed.hasher.finalize() != received.hasher.finalize() {
            return Err(SaplingRemoteApiError::IncompleteResponse(
                "Streaming clone response changed during retry".into(),
            ));
        }

        let received = self.received.clone();
        let entries = entries
            .map_ok(move |entry| {
                received.lock().push(&entry)?;
                Ok(entry)
            })
            .and_then(future::ready)
            .boxed();
        Ok(Response { entries, stats })
    }
}
