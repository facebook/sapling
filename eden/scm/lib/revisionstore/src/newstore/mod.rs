/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use futures::{
    future,
    stream::{self, BoxStream},
};

pub mod edenapi;

/// A pinned, boxed stream of keys to fetch.
pub type KeyStream<K> = BoxStream<'static, K>;

/// A pinned, boxed stream of (fallible) fetched values
pub type FetchStream<V> = BoxStream<'static, Result<V, Error>>;

/// Transform an error into a single-item FetchStream
pub fn fetch_error<V, E>(e: E) -> FetchStream<V>
where
    E: std::error::Error + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    Box::pin(stream::once(future::err(Error::new(e))))
}

// TODO: Add attributes support
/// A typed, async key-value storage API
#[async_trait]
pub trait ReadStore<K: Send + Sync + 'static, V: Send + Sync + 'static>:
    Send + Sync + 'static
{
    /// Map a stream of keys to a stream of values by fetching from the underlying store
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<V>;
}
