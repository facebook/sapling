/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use futures::{future::ok, stream::iter_ok};
use tokio::prelude::*;

use cloned::cloned;
use revisionstore::{HgIdHistoryStore, ToKeys};
use types::{Key, NodeInfo};

use crate::util::AsyncWrapper;

/// Allow a `HgIdHistoryStore` to be used in an asynchronous context
pub struct AsyncHgIdHistoryStore<T: HgIdHistoryStore> {
    history: AsyncWrapper<T>,
}

impl<T: HgIdHistoryStore + Send + Sync> AsyncHgIdHistoryStore<T> {
    pub(crate) fn new_(store: T) -> Self {
        AsyncHgIdHistoryStore {
            history: AsyncWrapper::new(store),
        }
    }

    /// Asynchronously call the HgIdHistoryStore::get_missing method.
    pub fn get_missing(
        &self,
        keys: Vec<Key>,
    ) -> impl Future<Item = Vec<Key>, Error = Error> + Send {
        self.history.block(move |store| store.get_missing(&keys))
    }

    /// Asynchronously call the HgIdHistoryStore::get_node_info method.
    pub fn get_node_info(
        &self,
        key: &Key,
    ) -> impl Future<Item = Option<NodeInfo>, Error = Error> + Send {
        cloned!(key);
        self.history.block(move |store| store.get_node_info(&key))
    }
}

impl<T: HgIdHistoryStore + ToKeys + Send + Sync> AsyncHgIdHistoryStore<T> {
    /// Iterate over all the keys of this historystore.
    pub fn iter(&self) -> impl Stream<Item = Key, Error = Error> + Send {
        let keysfut = self.history.block(move |store| Ok(store.to_keys()));
        keysfut
            .and_then(|keys: Vec<Result<Key>>| ok(iter_ok(keys.into_iter().flatten())))
            .flatten_stream()
    }
}
