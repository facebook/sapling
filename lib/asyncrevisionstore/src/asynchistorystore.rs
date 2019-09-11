// Copyright 2019 Facebook, Inc.

use failure::{Error, Fallible};
use futures::{future::ok, stream::iter_ok};
use tokio::prelude::*;

use cloned::cloned;
use revisionstore::{Ancestors, HistoryStore, ToKeys};
use types::{Key, NodeInfo};

use crate::util::AsyncWrapper;

/// Allow a `HistoryStore` to be used in an asynchronous context
pub struct AsyncHistoryStore<T: HistoryStore> {
    history: AsyncWrapper<T>,
}

impl<T: HistoryStore + Send + Sync> AsyncHistoryStore<T> {
    pub(crate) fn new_(store: T) -> Self {
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

impl<T: HistoryStore + ToKeys + Send + Sync> AsyncHistoryStore<T> {
    /// Iterate over all the keys of this historystore.
    pub fn iter(&self) -> impl Stream<Item = Key, Error = Error> + Send {
        let keysfut = self.history.block(move |store| Ok(store.to_keys()));
        keysfut
            .and_then(|keys: Vec<Fallible<Key>>| ok(iter_ok(keys.into_iter().flatten())))
            .flatten_stream()
    }
}
