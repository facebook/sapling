/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::From;
use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use futures::{channel::mpsc::channel, SinkExt, StreamExt, TryStreamExt};
use tracing::error;

use streams::select_drop;

use crate::newstore::{BoxedReadStore, BoxedWriteStore, FetchStream, KeyStream, ReadStore};

/// A combinator which queries a preferred store, then falls back to a fallback store
/// if a key is not found in the preferred store.
pub struct FallbackStore<K, VP, VF> {
    /// The preferred store, which will always be queried. Usually a local store.
    pub preferred: BoxedReadStore<K, VP>,

    /// The fallback store, which will be queried if the value is not found in the
    /// primary store.
    pub fallback: BoxedReadStore<K, VF>,

    // TODO(meyer): Should we make a `BoxedRwStore` that explicitly does both?
    /// A `WriteStore` to which values read from the fallback store are written. Generally
    /// this will be the same as the preferred store.
    pub write_store: BoxedWriteStore<K, VP>,

    /// If `write` is true, values read from the fallback store will be written to `write_store`
    pub write: bool,
}

const CHANNEL_BUFFER: usize = 200;

#[async_trait]
impl<K, VP, VF> ReadStore<K, VP> for FallbackStore<K, VP, VF>
where
    K: Send + Sync + Clone + Unpin + 'static,
    VF: Send + Sync + 'static,
    VP: Send + Sync + Clone + From<VF> + 'static,
{
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, VP> {
        // TODO(meyer): Write a custom Stream implementation to try to avoid use of channels
        let (sender, receiver) = channel(CHANNEL_BUFFER);

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
                                // TODO(meyer): We should really only fallback for "not found" errors, and pass through others.
                                // Otherwise swallowing this error here could lose us valuable information.
                                // TODO(meyer): Looks like we aren't up to date with futures crate, missing "feed" method, which is probably better here.
                                // I think this might serialize the fallback stream as-written.
                                match sender.send(k.clone()).await {
                                    Ok(()) => None,
                                    Err(e) => Some(Err((Some(k), e.into()))),
                                }
                            }
                        }
                    }
                });

        let (write_sender, write_receiver) = channel(CHANNEL_BUFFER);

        let write_fallbacks = self.write;
        let fallback_stream = self
            .fallback
            .clone()
            .fetch_stream(Box::pin(receiver))
            .await
            .map_ok(|v| v.into())
            .and_then(move |v: VP| {
                let mut write_sender = write_sender.clone();
                async move {
                    if write_fallbacks {
                        if let Err(e) = write_sender.send(v.clone()).await {
                            // TODO(meyer): Eventually add better tracing support to these traits. Each combinator should have a span, etc.
                            // TODO(meyer): Update tracing? Looks like we don't have access to the most recent version of the macro syntax.
                            error!({ error = %e }, "error writing fallback value to channel");
                        }
                    }
                    Ok(v)
                }
            });

        // TODO(meyer): This whole "fake filter map" approach to driving the write stream forward seems bad.
        let write_results_null = self
            .write_store
            .clone()
            .write_stream(Box::pin(write_receiver))
            .await
            // TODO(meyer): Don't swallow all write errors here.
            .filter_map(|_res| {
                futures::future::ready(Option::<Result<VP, (Option<K>, Error)>>::None)
            });

        // TODO(meyer): Implement `select_all_drop` if we continue with this approach
        Box::pin(select_drop(
            preferred_stream,
            select_drop(fallback_stream, write_results_null),
        ))
    }
}
