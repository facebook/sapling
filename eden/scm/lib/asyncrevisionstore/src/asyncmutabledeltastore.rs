/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Error;
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;
use revisionstore::{Delta, HgIdMutableDeltaStore, Metadata};

pub struct AsyncHgIdMutableDeltaStore<T: HgIdMutableDeltaStore> {
    inner: Option<T>,
}

/// Wraps a HgIdMutableDeltaStore to be used in an asynchronous context.
///
/// The API is designed to consume the `AsyncHgIdMutableDeltaStore` and return it, this allows chaining
/// the Futures with `and_then()`.
///
/// # Examples
/// ```
/// let mutablepack = AsyncMutableDataPack::new(path, DataPackVersion::One);
/// let work = mutablepack
///     .and_then(move |datapack| datapack.add(&delta1, &meta1))
///     .and_then(move |datapack| datapack.add(&delta2, &meta2))
///     .and_then(move |datapack| datapack.close()
/// ```
impl<T: HgIdMutableDeltaStore + Send> AsyncHgIdMutableDeltaStore<T> {
    pub(crate) fn new_(store: T) -> Self {
        AsyncHgIdMutableDeltaStore { inner: Some(store) }
    }

    /// Add the `Delta` to this store.
    pub fn add(
        mut self,
        delta: &Delta,
        metadata: &Metadata,
    ) -> impl Future<Item = Self, Error = Error> + Send {
        poll_fn({
            cloned!(delta, metadata);
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let inner = inner.expect("The delta store is closed");
                    inner.add(&delta, &metadata).map(|()| inner)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |inner| AsyncHgIdMutableDeltaStore { inner: Some(inner) })
    }

    /// Close the store. Once this Future finishes, all the added delta becomes visible to other
    /// processes.
    pub fn close(mut self) -> impl Future<Item = Option<PathBuf>, Error = Error> + Send {
        poll_fn({
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let inner = inner.expect("The delta store is closed");
                    inner.flush()
                })
            }
        })
        .from_err()
        .and_then(|res| res)
    }
}
