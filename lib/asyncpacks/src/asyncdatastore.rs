// Copyright 2019 Facebook, Inc.

use std::sync::{Arc, Mutex};

use failure::{Error, Fallible};
use futures::future::poll_fn;
use tokio::prelude::*;
use tokio_threadpool::blocking;

use cloned::cloned;
use revisionstore::{key::Key, DataStore, Delta, Metadata};

struct AsyncDataStoreInner<T: DataStore> {
    data: T,
}

/// Allow a `DataStore` to be used in an asynchronous context
pub struct AsyncDataStore<T: DataStore> {
    inner: Arc<Mutex<AsyncDataStoreInner<T>>>,
}

impl<T: DataStore + Send> AsyncDataStore<T> {
    pub(crate) fn new(store: T) -> Self {
        AsyncDataStore {
            inner: Arc::new(Mutex::new(AsyncDataStoreInner { data: store })),
        }
    }

    /// Helper that calls `callback` in asynchronous context.
    fn block<U: Send>(
        &self,
        callback: impl Fn(&T) -> Fallible<U> + Send,
    ) -> impl Future<Item = U, Error = Error> + Send {
        poll_fn({
            cloned!(self.inner);
            move || {
                blocking(|| {
                    let inner = inner.lock().expect("Poisoned Mutex");
                    callback(&inner.data)
                })
            }
        })
        .from_err()
        .and_then(|res| res)
    }

    /// Asynchronously call the DataStore::get method.
    pub fn get(&self, key: &Key) -> impl Future<Item = Vec<u8>, Error = Error> + Send {
        cloned!(key);
        self.block(move |store| store.get(&key))
    }

    /// Asynchronously call the DataStore::get_delta method.
    pub fn get_delta(&self, key: &Key) -> impl Future<Item = Delta, Error = Error> + Send {
        cloned!(key);
        self.block(move |store| store.get_delta(&key))
    }

    /// Asynchronously call the DataStore::get_delta_chain method.
    pub fn get_delta_chain(
        &self,
        key: &Key,
    ) -> impl Future<Item = Vec<Delta>, Error = Error> + Send {
        cloned!(key);
        self.block(move |store| store.get_delta_chain(&key))
    }

    /// Asynchronously call the DataStore::get_meta method.
    pub fn get_meta(&self, key: &Key) -> impl Future<Item = Metadata, Error = Error> + Send {
        cloned!(key);
        self.block(move |store| store.get_meta(&key))
    }

    /// Asynchronously call the DataStore::get_missing method.
    pub fn get_missing(
        &self,
        keys: &'static [Key],
    ) -> impl Future<Item = Vec<Key>, Error = Error> + Send {
        self.block(move |store| store.get_missing(keys))
    }
}
