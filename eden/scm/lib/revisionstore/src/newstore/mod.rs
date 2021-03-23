/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{fmt, sync::Arc};

use anyhow::Error;
use async_trait::async_trait;
use futures::{
    future,
    stream::{self, BoxStream},
};
use thiserror::Error;

pub use self::{
    edenapi::EdenApiAdapter,
    fallback::{Fallback, FallbackCache},
    legacy::LegacyDatastore,
};

pub mod edenapi;
pub mod fallback;
pub mod legacy;

/// A pinned, boxed stream of keys to fetch.
pub type KeyStream<K> = BoxStream<'static, K>;

/// A pinned, boxed stream of (fallible) fetched values
pub type FetchStream<K, V> = BoxStream<'static, Result<V, FetchError<K>>>;

/// A boxed, object-safe ReadStore trait object for a given key and value type.
pub type BoxedReadStore<K, V> = Arc<dyn ReadStore<K, V>>;

pub type WriteStream<V> = BoxStream<'static, V>;

pub type WriteResults<K> = BoxStream<'static, Result<K, (Option<K>, Error)>>;

pub type BoxedWriteStore<K, V> = Arc<dyn WriteStore<K, V>>;

#[derive(Debug, Error)]
pub enum FetchError<K: fmt::Debug + fmt::Display> {
    #[error("failed to fetch key '{0}': key not found")]
    NotFound(K),

    #[error("failed to fetch key '{0}': {1}")]
    KeyedError(K, Error),

    #[error(transparent)]
    Other(#[from] Error),
}

impl<K> FetchError<K>
where
    K: fmt::Debug + fmt::Display,
{
    pub fn not_found(key: K) -> Self {
        FetchError::NotFound(key)
    }

    pub fn with_key(key: K, err: impl Into<Error>) -> Self {
        FetchError::KeyedError(key, err.into())
    }

    pub fn maybe_with_key(maybe_key: Option<K>, err: impl Into<Error>) -> Self {
        match maybe_key {
            Some(key) => FetchError::KeyedError(key, err.into()),
            None => FetchError::Other(err.into()),
        }
    }

    pub fn from(err: impl Into<Error>) -> Self {
        FetchError::Other(err.into())
    }
}

/// Transform an error into a single-item FetchStream
pub fn fetch_error<K, V, E>(e: E) -> FetchStream<K, V>
where
    E: Into<Error> + Send + Sync + 'static,
    K: fmt::Display + fmt::Debug + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    Box::pin(stream::once(future::ready(Err(FetchError::from(e)))))
}

// TODO: Add attributes support
/// A typed, async key-value storage API
#[async_trait]
pub trait ReadStore<K: fmt::Display + fmt::Debug + Send + Sync + 'static, V: Send + Sync + 'static>:
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
