/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::From;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{channel::mpsc::channel, SinkExt, StreamExt, TryStreamExt};

use streams::select_drop;

use crate::newstore::{BoxedReadStore, FetchStream, KeyStream, ReadStore};

/// A combinator which queries a preferred store, then falls back to a fallback store
/// if a key is not found in the preferred store.
pub struct FallbackStore<K, VP, VF> {
    /// The preferred store, which will always be queried. Usually a local store.
    pub preferred: BoxedReadStore<K, VP>,

    /// The fallback store, which will be queried if the value is not found in the
    /// primary store.
    pub fallback: BoxedReadStore<K, VF>,
}

const CHANNEL_BUFFER: usize = 200;

#[async_trait]
impl<K, VP, VF> ReadStore<K, VP> for FallbackStore<K, VP, VF>
where
    K: Send + Sync + Clone + Unpin + 'static,
    VF: Send + Sync + 'static,
    VP: Send + Sync + From<VF> + 'static,
{
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, VP> {
        let (sender, receiver) = channel(CHANNEL_BUFFER);
        let receiver = Box::pin(receiver);

        let preferred_stream =
            self.preferred
                .clone()
                .fetch_stream(keys)
                .await
                .filter_map(move |res| {
                    let mut sender = sender.clone();
                    async move {
                        match res {
                            Ok(v) => Some(Ok(v)),
                            Err((None, e)) => Some(Err((None, e))),
                            Err((Some(k), _e)) => {
                                // TODO: We should really only fallback for "not found" errors, and pass through others.
                                // Otherwise swallowing this error here could lose us valuable information.
                                // TODO: Looks like we aren't up to date with futures crate, missing "feed" method, which is probably better here.
                                // I think this might serialize the fallback stream as-written.
                                match sender.send(k.clone()).await {
                                    Ok(()) => None,
                                    Err(e) => Some(Err((Some(k), e.into()))),
                                }
                            }
                        }
                    }
                });

        let fallback_stream = self
            .fallback
            .clone()
            .fetch_stream(receiver)
            .await
            .map_ok(move |v| v.into());

        Box::pin(select_drop(preferred_stream, fallback_stream))
    }
}
