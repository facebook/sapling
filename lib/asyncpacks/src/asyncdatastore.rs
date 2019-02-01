// Copyright 2019 Facebook, Inc.

use failure::Error;
use tokio::prelude::*;

use cloned::cloned;
use revisionstore::{key::Key, DataStore, Delta, Metadata};

use crate::util::AsyncWrapper;

/// Allow a `DataStore` to be used in an asynchronous context
pub struct AsyncDataStore<T: DataStore> {
    data: AsyncWrapper<T>,
}

impl<T: DataStore + Send> AsyncDataStore<T> {
    pub(crate) fn new(store: T) -> Self {
        AsyncDataStore {
            data: AsyncWrapper::new(store),
        }
    }

    /// Asynchronously call the DataStore::get method.
    pub fn get(&self, key: &Key) -> impl Future<Item = Vec<u8>, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get(&key))
    }

    /// Asynchronously call the DataStore::get_delta method.
    pub fn get_delta(&self, key: &Key) -> impl Future<Item = Delta, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get_delta(&key))
    }

    /// Asynchronously call the DataStore::get_delta_chain method.
    pub fn get_delta_chain(
        &self,
        key: &Key,
    ) -> impl Future<Item = Vec<Delta>, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get_delta_chain(&key))
    }

    /// Asynchronously call the DataStore::get_meta method.
    pub fn get_meta(&self, key: &Key) -> impl Future<Item = Metadata, Error = Error> + Send {
        cloned!(key);
        self.data.block(move |store| store.get_meta(&key))
    }

    /// Asynchronously call the DataStore::get_missing method.
    pub fn get_missing(
        &self,
        keys: &'static [Key],
    ) -> impl Future<Item = Vec<Key>, Error = Error> + Send {
        self.data.block(move |store| store.get_missing(keys))
    }
}
