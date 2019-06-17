// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    path::PathBuf,
};

use failure::Fallible;

use types::{HistoryEntry, Key, NodeInfo};

use crate::localstore::LocalStore;

pub type Ancestors = HashMap<Key, NodeInfo>;

pub trait HistoryStore: LocalStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors>;
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo>;
}

pub trait MutableHistoryStore {
    fn add(&mut self, key: &Key, info: &NodeInfo) -> Fallible<()>;
    fn flush(&mut self) -> Fallible<Option<PathBuf>>;

    fn add_entry(&mut self, entry: &HistoryEntry) -> Fallible<()> {
        self.add(&entry.key, &entry.nodeinfo)
    }
}

/// Implement `HistoryStore` for all types that can be `Deref` into a `HistoryStore`.
impl<T: HistoryStore + ?Sized, U: Deref<Target = T>> HistoryStore for U {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        T::get_ancestors(self, key)
    }
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        T::get_node_info(self, key)
    }
}

impl<T: MutableHistoryStore + ?Sized, U: DerefMut<Target = T>> MutableHistoryStore for U {
    fn add(&mut self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        T::add(self, key, info)
    }

    fn flush(&mut self) -> Fallible<Option<PathBuf>> {
        T::flush(self)
    }
}
