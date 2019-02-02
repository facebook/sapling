// Copyright 2019 Facebook, Inc.

use failure::Error;
use tokio::prelude::*;

use cloned::cloned;
use revisionstore::{Ancestors, HistoryStore, Key, NodeInfo};

use crate::util::AsyncWrapper;

/// Allow a `HistoryStore` to be used in an asynchronous context
pub struct AsyncHistoryStore<T: HistoryStore> {
    history: AsyncWrapper<T>,
}

impl<T: HistoryStore + Send> AsyncHistoryStore<T> {
    pub(crate) fn new(store: T) -> Self {
        AsyncHistoryStore {
            history: AsyncWrapper::new(store),
        }
    }

    /// Asynchronously call the HistoryStore::get_ancestors method.
    pub fn get_ancestors(&self, key: &Key) -> impl Future<Item = Ancestors, Error = Error> + Send {
        cloned!(key);
        self.history.block(move |store| store.get_ancestors(&key))
    }

    /// Asynchronously call the HistoryStore::get_missing method.
    pub fn get_missing(
        &self,
        keys: Vec<Key>,
    ) -> impl Future<Item = Vec<Key>, Error = Error> + Send {
        self.history.block(move |store| store.get_missing(&keys))
    }

    /// Asynchronously call the HistoryStore::get_node_info method.
    pub fn get_node_info(&self, key: &Key) -> impl Future<Item = NodeInfo, Error = Error> + Send {
        cloned!(key);
        self.history.block(move |store| store.get_node_info(&key))
    }
}
