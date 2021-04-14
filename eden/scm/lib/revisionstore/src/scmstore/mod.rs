/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{cmp::PartialEq, fmt, sync::Arc};

use anyhow::Error;
use futures::{
    future,
    stream::{self, BoxStream},
};
use thiserror::Error;

pub use self::{
    builder::{FileScmStoreBuilder, TreeScmStoreBuilder},
    edenapi::EdenApiAdapter,
    fallback::{Fallback, FallbackCache},
    filter_map::FilterMapStore,
    inmemory::{HashMapStore, KeyedValue},
    legacy::LegacyDatastore,
    types::{StoreFile, StoreTree},
};

pub mod builder;
pub mod edenapi;
pub mod fallback;
pub mod filter_map;
pub mod inmemory;
pub mod legacy;
pub mod lfs;
pub mod types;
pub mod util;

/// A pinned, boxed stream of keys to fetch.
pub type KeyStream<K> = BoxStream<'static, K>;

/// A pinned, boxed stream of (fallible) fetched values
pub type FetchStream<K, V> = BoxStream<'static, Result<V, FetchError<K>>>;

/// A ReadStore trait object for a given key and value type.
pub type BoxedReadStore<K, V> = Arc<dyn ReadStore<K, V>>;

/// A pinned, boxed stream of values to write.
pub type WriteStream<V> = BoxStream<'static, V>;

/// A pinned, boxed stream of (fallible) write results.
pub type WriteResults<K> = BoxStream<'static, Result<K, WriteError<K>>>;

/// A WriteStore trait object for a given key and value type.
pub type BoxedWriteStore<K, V> = Arc<dyn WriteStore<K, V>>;

/// A trait object for stores which support both reading and writing.
pub type BoxedRWStore<K, V> = Arc<dyn ReadWriteStore<K, V>>;

// Automatic blanket impls for FetchKey and FetchValue. For now they're just used to simplify trait bounds.
pub trait FetchKey: fmt::Display + fmt::Debug + Clone + Send + Sync + 'static {}
impl<K> FetchKey for K where K: fmt::Display + fmt::Debug + Clone + Send + Sync + 'static {}

pub trait FetchValue: Send + Sync + Clone + 'static {}
impl<V> FetchValue for V where V: Send + Sync + Clone + 'static {}

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

impl<K> PartialEq for FetchError<K>
where
    K: PartialEq + fmt::Debug + fmt::Display,
{
    fn eq(&self, other: &Self) -> bool {
        use FetchError::*;
        match (self, other) {
            (NotFound(k1), NotFound(k2)) => k1 == k2,
            _ => false,
        }
    }
}

/// Transform an error into a single-item FetchStream
pub fn fetch_error<K, V, E>(e: E) -> FetchStream<K, V>
where
    E: Into<Error> + Send + Sync + 'static,
    K: FetchKey,
    V: FetchValue,
{
    Box::pin(stream::once(future::ready(Err(FetchError::from(e)))))
}

// TODO(meyer): We'll likely want to make it WriteError::KeyValueError(K, V, Error) eventually so we can have write fallbacks.
#[derive(Debug, Error)]
pub enum WriteError<K: fmt::Debug + fmt::Display> {
    /// Write failed with an error
    #[error("failed to write key '{0}': {1}")]
    KeyedError(K, Error),

    /// Write failure not specific to a particular key
    #[error(transparent)]
    Other(#[from] Error),
}

impl<K> WriteError<K>
where
    K: fmt::Debug + fmt::Display,
{
    pub fn with_key(key: K, err: impl Into<Error>) -> Self {
        WriteError::KeyedError(key, err.into())
    }

    pub fn from(err: impl Into<Error>) -> Self {
        WriteError::Other(err.into())
    }
}

/// A typed, key-value store which supports both reading and writing.
pub trait ReadWriteStore<K: FetchKey, V: FetchValue>: ReadStore<K, V> + WriteStore<K, V> {}

impl<T, K, V> ReadWriteStore<K, V> for T
where
    K: FetchKey,
    V: FetchValue,
    T: ReadStore<K, V> + WriteStore<K, V>,
{
}

// TODO: Add attributes support
/// A typed, async key-value storage API
pub trait ReadStore<K: FetchKey, V: FetchValue>: Send + Sync + 'static {
    /// Map a stream of keys to a stream of values by fetching from the underlying store
    fn fetch_stream(self: Arc<Self>, keys: KeyStream<K>) -> FetchStream<K, V>;
}

// TODO: Add attributes support
/// A typed, async key-value storage API
pub trait WriteStore<K: FetchKey, V: FetchValue>: Send + Sync + 'static {
    fn write_stream(self: Arc<Self>, values: WriteStream<V>) -> WriteResults<K>;
}
