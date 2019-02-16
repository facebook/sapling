// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{collections::HashMap, rc::Rc, sync::Arc};

use failure::Fallible;

use types::{Key, Node, NodeInfo};

pub type Ancestors = HashMap<Key, NodeInfo>;

pub trait HistoryStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors>;
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>>;
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo>;
}

impl<T: HistoryStore> HistoryStore for Rc<T> {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        T::get_ancestors(self, key)
    }
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        T::get_missing(self, keys)
    }
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        T::get_node_info(self, key)
    }
}

impl<T: HistoryStore> HistoryStore for Arc<T> {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        T::get_ancestors(self, key)
    }
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        T::get_missing(self, keys)
    }
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        T::get_node_info(self, key)
    }
}

impl<T: HistoryStore> HistoryStore for Box<T> {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        T::get_ancestors(self, key)
    }
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        T::get_missing(self, keys)
    }
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        T::get_node_info(self, key)
    }
}
