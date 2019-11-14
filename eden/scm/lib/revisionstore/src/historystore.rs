/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{ops::Deref, path::PathBuf};

use failure::Fallible;

use types::{HistoryEntry, Key, NodeInfo};

use crate::localstore::LocalStore;

pub trait HistoryStore: LocalStore + Send + Sync {
    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>>;
}

pub trait MutableHistoryStore: HistoryStore + Send + Sync {
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()>;
    fn flush(&self) -> Fallible<Option<PathBuf>>;

    fn add_entry(&self, entry: &HistoryEntry) -> Fallible<()> {
        self.add(&entry.key, &entry.nodeinfo)
    }
}

/// The `RemoteHistoryStore` trait indicates that data can fetched over the network. Care must be
/// taken to avoid serially fetching data and instead data should be fetched in bulk via the
/// `prefetch` API.
pub trait RemoteHistoryStore: HistoryStore + Send + Sync {
    /// Attempt to bring the data corresponding to the passed in keys to a local store.
    ///
    /// When implemented on a pure remote store, like the `EdenApi`, the method will always fetch
    /// everything that was asked. On a higher level store, such as the `MetadataStore`, this will
    /// avoid fetching data that is already present locally.
    fn prefetch(&self, keys: Vec<Key>) -> Fallible<()>;
}

/// Implement `HistoryStore` for all types that can be `Deref` into a `HistoryStore`.
impl<T: HistoryStore + ?Sized, U: Deref<Target = T> + Send + Sync> HistoryStore for U {
    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        T::get_node_info(self, key)
    }
}

impl<T: MutableHistoryStore + ?Sized, U: Deref<Target = T> + Send + Sync> MutableHistoryStore
    for U
{
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        T::add(self, key, info)
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        T::flush(self)
    }
}
