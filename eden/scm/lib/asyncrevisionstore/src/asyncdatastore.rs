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
use revisionstore::{Delta, HgIdDataStore, Metadata, ToKeys};
use types::Key;

use crate::util::AsyncWrapper;

/// Allow a `HgIdDataStore` to be used in an asynchronous context
pub struct AsyncHgIdDataStore<T: HgIdDataStore> {
    data: AsyncWrapper<T>,
}

impl<T: HgIdDataStore + Send + Sync> AsyncHgIdDataStore<T> {
    pub(crate) fn new_(store: T) -> Self {
        AsyncHgIdDataStore {
            data: AsyncWrapper::new(store),
        }
    }

    /// Asynchronously call the HgIdDataStore::get method.
    pub fn get(&self, key: &Key) -> impl Future<Item = Option<Vec<u8>>, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get(&key))
    }

    /// Asynchronously call the HgIdDataStore::get_delta method.
    pub fn get_delta(&self, key: &Key) -> impl Future<Item = Option<Delta>, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get_delta(&key))
    }

    /// Asynchronously call the HgIdDataStore::get_delta_chain method.
    pub fn get_delta_chain(
        &self,
        key: &Key,
    ) -> impl Future<Item = Option<Vec<Delta>>, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get_delta_chain(&key))
    }

    /// Asynchronously call the HgIdDataStore::get_meta method.
    pub fn get_meta(
        &self,
        key: &Key,
    ) -> impl Future<Item = Option<Metadata>, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get_meta(&key))
    }

    /// Asynchronously call the HgIdDataStore::get_missing method.
    pub fn get_missing(
        &self,
        keys: &'static [Key],
    ) -> impl Future<Item = Vec<Key>, Error = Error> + Send {
        self.data.block(move |store| store.get_missing(keys))
    }
}

impl<T: HgIdDataStore + ToKeys + Send + Sync> AsyncHgIdDataStore<T> {
    /// Iterate over all the keys of this datastore.
    pub fn iter(&self) -> impl Stream<Item = Key, Error = Error> + Send {
        let keysfut = self.data.block(move |store| Ok(store.to_keys()));
        keysfut
            .and_then(|keys: Vec<Result<Key>>| ok(iter_ok(keys.into_iter().flatten())))
            .flatten_stream()
    }
}
