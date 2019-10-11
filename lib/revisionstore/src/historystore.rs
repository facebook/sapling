// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{collections::HashMap, ops::Deref, path::PathBuf};

use failure::Fallible;

use types::{HistoryEntry, Key, NodeInfo};

use crate::localstore::LocalStore;

pub type Ancestors = HashMap<Key, NodeInfo>;

pub trait HistoryStore: LocalStore + Send + Sync {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>>;
    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>>;
}

pub trait MutableHistoryStore: HistoryStore + Send + Sync {
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()>;
    fn flush(&self) -> Fallible<Option<PathBuf>>;

    fn add_entry(&self, entry: &HistoryEntry) -> Fallible<()> {
        self.add(&entry.key, &entry.nodeinfo)
    }
}

/// Implement `HistoryStore` for all types that can be `Deref` into a `HistoryStore`.
impl<T: HistoryStore + ?Sized, U: Deref<Target = T> + Send + Sync> HistoryStore for U {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        T::get_ancestors(self, key)
    }
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
