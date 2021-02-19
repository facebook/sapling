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
pub mod fallback;
pub mod legacy;

/// A pinned, boxed stream of keys to fetch.
pub type KeyStream<K> = BoxStream<'static, K>;

/// A pinned, boxed stream of (fallible) fetched values
pub type FetchStream<K, V> = BoxStream<'static, Result<V, (Option<K>, Error)>>;

/// A boxed, object-safe ReadStore trait object for a given key and value type.
pub type BoxedReadStore<K, V> = Arc<dyn ReadStore<K, V>>;

pub type WriteStream<V> = BoxStream<'static, V>;

pub type WriteResults<K> = BoxStream<'static, Result<K, (Option<K>, Error)>>;

pub type BoxedWriteStore<K, V> = Arc<dyn WriteStore<K, V>>;

/// Transform an error into a single-item FetchStream
pub fn fetch_error<K, V, E>(e: E) -> FetchStream<K, V>
where
    E: std::error::Error + Send + Sync + 'static,
    K: Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    Box::pin(stream::once(future::ready(Err((None, Error::new(e))))))
}

// TODO: Add attributes support
/// A typed, async key-value storage API
#[async_trait]
pub trait ReadStore<K: Send + Sync + 'static, V: Send + Sync + 'static>:
    Send + Sync + 'static
{
    /// Map a stream of keys to a stream of values by fetching from the underlying store
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, V>;
}

// TODO: Add attributes support
/// A typed, async key-value storage API
#[async_trait]
pub trait WriteStore<K: Send + Sync + 'static, V: Send + Sync + 'static>:
    Send + Sync + 'static
{
    async fn write_stream(self: Arc<Self>, values: WriteStream<V>) -> WriteResults<K>;
}
