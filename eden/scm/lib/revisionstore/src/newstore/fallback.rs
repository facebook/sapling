/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::{From, Into, TryFrom, TryInto};
use std::fmt;
use std::sync::Arc;

use anyhow::Error;
use futures::{channel::mpsc::channel, SinkExt, StreamExt, TryStreamExt};
use tracing::error;

use streams::select_drop;

use crate::newstore::{
    BoxedReadStore, BoxedWriteStore, FetchError, FetchStream, KeyStream, ReadStore,
};

/// A combinator which queries a preferred store, then falls back to a fallback store
/// if a key is not found in the preferred store. Keys fetched from the fallback will
/// be written to the write_store if available.
pub struct FallbackCache<K, VP, VF, VW> {
    /// The preferred store, which will always be queried. Usually a local store.
    pub preferred: BoxedReadStore<K, VP>,

    /// The fallback store, which will be queried if the value is not found in the
    /// primary store.
    pub fallback: BoxedReadStore<K, VF>,

    /// A `WriteStore` to which values read from the fallback store are written. Generally
    /// this will be the same as the preferred store.
    pub write_store: Option<BoxedWriteStore<K, VW>>,
}

const CHANNEL_BUFFER: usize = 200;

impl<K, VP, VF, VW, VO> ReadStore<K, VO> for FallbackCache<K, VP, VF, VW>
where
    K: fmt::Display + fmt::Debug + Send + Sync + Clone + Unpin + 'static,
    // Preferred Value Type
    VP: Send + Sync + Clone + 'static,
    // Fallback Value Type
    VF: Send + Sync + Clone + 'static,
    // Write Value Type (must support conversion from fallback)
    VW: Send + Sync + Clone + From<VF> + 'static,
    // Output Value Type (must support conversion from preferred & fallback)
    VO: Send + Sync + Clone + TryFrom<VF> + TryFrom<VP> + 'static,
    // TODO(meyer): For now, we just require the conversion errors to convertible to anyhow::Error
    // We can probably loosen this later. In particular, we want to associate the key, at least.
    <VO as TryFrom<VF>>::Error: Into<Error>,
    <VO as TryFrom<VP>>::Error: Into<Error>,
{
    fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, VO> {
        // TODO(meyer): Write a custom Stream implementation to try to avoid use of channels
        let (sender, receiver) = channel(CHANNEL_BUFFER);

        let preferred_stream = self
            .preferred
            .clone()
            .fetch_stream(keys)
            .filter_map(move |res| {
                let mut sender = sender.clone();
                async move {
                    use FetchError::*;
                    match res {
                        // Convert preferred values into output values
                        Ok(v) => Some(v.try_into().map_err(FetchError::from)),
                        // TODO(meyer): Looks like we aren't up to date with futures crate, missing "feed" method, which is probably better here.
                        // I think this might serialize the fallback stream as-written.
                        Err(NotFound(k)) => match sender.send(k.clone()).await {
                            Ok(()) => None,
                            Err(e) => Some(Err(FetchError::with_key(k, e))),
                        },
                        // TODO(meyer): Should we also fall back on KeyedError, but also log an error?
                        Err(e) => Some(Err(e)),
                    }
                }
            });

        let fallback_stream = self.fallback.clone().fetch_stream(Box::pin(receiver));

        if let Some(ref write_store) = self.write_store {
            let (write_sender, write_receiver) = channel(CHANNEL_BUFFER);

            let fallback_stream = fallback_stream.and_then(move |v: VF| {
                let mut write_sender = write_sender.clone();
                async move {
                    // Convert fallback values to write values
                    if let Err(e) = write_sender.send(v.clone().into()).await {
                        // TODO(meyer): Eventually add better tracing support to these traits. Each combinator should have a span, etc.
                        // TODO(meyer): Update tracing? Looks like we don't have access to the most recent version of the macro syntax.
                        error!({ error = %e }, "error writing fallback value to channel");
                    }
                    // Convert fallback values to output values
                    v.try_into().map_err(FetchError::from)
                }
            });

            // TODO(meyer): This whole "fake filter map" approach to driving the write stream forward seems bad.
            let write_results_null = write_store
                .clone()
                .write_stream(Box::pin(write_receiver))
                // TODO(meyer): Don't swallow all write errors here.
                .filter_map(|_res| futures::future::ready(None));

            // TODO(meyer): Implement `select_all_drop` if we continue with this approach
            Box::pin(select_drop(
                preferred_stream,
                select_drop(fallback_stream, write_results_null),
            ))
        } else {
            // Convert fallback values to output values
            Box::pin(select_drop(
                preferred_stream,
                fallback_stream.map(|r| r.and_then(|v| v.try_into().map_err(FetchError::from))),
            ))
        }
    }
}

/// A combinator which queries a preferred store, then falls back to a fallback store
/// if a key is not found in the preferred store. Unlike `FallbackCache`, this type
/// does not support writing. It is provided for cases where writing is not desired, and
/// requiring a conversion to the write value type is nonsensical.
pub struct Fallback<K, VP, VF> {
    /// The preferred store, which will always be queried. Usually a local store.
    pub preferred: BoxedReadStore<K, VP>,

    /// The fallback store, which will be queried if the value is not found in the
    /// primary store.
    pub fallback: BoxedReadStore<K, VF>,
}

impl<K, VP, VF, VO> ReadStore<K, VO> for Fallback<K, VP, VF>
where
    K: fmt::Display + fmt::Debug + Send + Sync + Clone + Unpin + 'static,
    // Preferred Value Type
    VP: Send + Sync + Clone + 'static,
    // Fallback Value Type
    VF: Send + Sync + Clone + 'static,
    // Output Value Type (must support conversion from preferred & fallback)
    VO: Send + Sync + Clone + From<VF> + From<VP> + 'static,
{
    fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, VO> {
        // TODO(meyer): Write a custom Stream implementation to try to avoid use of channels
        let (sender, receiver) = channel(CHANNEL_BUFFER);

        let preferred_stream = self
            .preferred
            .clone()
            .fetch_stream(keys)
            .filter_map(move |res| {
                let mut sender = sender.clone();
                async move {
                    use FetchError::*;
                    match res {
                        // Convert preferred values into output values
                        Ok(v) => Some(Ok(v.into())),
                        // TODO(meyer): Looks like we aren't up to date with futures crate, missing "feed" method, which is probably better here.
                        // I think this might serialize the fallback stream as-written.
                        Err(NotFound(k)) => match sender.send(k.clone()).await {
                            Ok(()) => None,
                            Err(e) => Some(Err(FetchError::with_key(k, e))),
                        },
                        // TODO(meyer): Should we also fall back on KeyedError, but also log an error?
                        Err(e) => Some(Err(e)),
                    }
                }
            });

        let fallback_stream = self
            .fallback
            .clone()
            .fetch_stream(Box::pin(receiver))
            .map_ok(|v| v.into());

        Box::pin(select_drop(preferred_stream, fallback_stream))
    }
}
