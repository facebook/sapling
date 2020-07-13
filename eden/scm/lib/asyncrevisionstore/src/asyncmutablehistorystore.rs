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
use edenapi_types::HistoryEntry;
use revisionstore::HgIdMutableHistoryStore;
use types::{Key, NodeInfo};

pub struct AsyncHgIdMutableHistoryStore<T: HgIdMutableHistoryStore> {
    inner: Option<T>,
}

/// Wraps a HgIdMutableHistoryStore to be used in an asynchronous context.
///
/// The API is designed to consume the `AsyncHgIdMutableHistoryStore` and return it, this allows chaining
/// the Futures with `and_then()`.
///
/// # Examples
/// ```
/// let mutablepack = AsyncMutableHistoryPack::new(path, HistoryPackVersion::One);
/// let work = mutablepack
///     .and_then(move |datapack| datapack.add_entry(&entry1))
///     .and_then(move |datapack| datapack.add_entry(&entry2))
///     .and_then(move |datapack| datapack.close()
/// ```
impl<T: HgIdMutableHistoryStore + Send> AsyncHgIdMutableHistoryStore<T> {
    pub(crate) fn new_(store: T) -> Self {
        AsyncHgIdMutableHistoryStore { inner: Some(store) }
    }

    /// Add the `NodeInfo` to this store.
    pub fn add(
        mut self,
        key: &Key,
        info: &NodeInfo,
    ) -> impl Future<Item = Self, Error = Error> + Send {
        poll_fn({
            cloned!(key, info);
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let inner = inner.expect("The history store is closed");
                    inner.add(&key, &info).map(|()| inner)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
        .map(move |inner| AsyncHgIdMutableHistoryStore { inner: Some(inner) })
    }

    /// Convenience function for adding a `types::PackHistoryEntry`.
    pub fn add_entry(self, entry: &HistoryEntry) -> impl Future<Item = Self, Error = Error> + Send {
        self.add(&entry.key, &entry.nodeinfo)
    }

    /// Close the store. Once this Future finishes, all the added `NodeInfo` becomes visible to
    /// other processes.
    pub fn close(mut self) -> impl Future<Item = Option<Vec<PathBuf>>, Error = Error> + Send {
        poll_fn({
            move || {
                blocking(|| {
                    let inner = self.inner.take();
                    let inner = inner.expect("The history store is closed");
                    inner.flush()
                })
            }
        })
        .from_err()
        .and_then(|res| res)
    }
}
