/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common method shared between all mercurial stores

use std::{ops::Deref, path::Path};

use anyhow::Result;

use crate::types::StoreKey;

pub trait LocalStore: Send + Sync {
    /// Builds a Store from a filepath. The default implementation panics.
    fn from_path(_path: &Path) -> Result<Self>
    where
        Self: Sized,
    {
        unimplemented!("Can't build a Store");
    }

    /// Returns all the keys that aren't present in this `Store`.
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>>;

    /// LFS is special, its data is split in 2 halves, a pointer that describes the blob, and an
    /// actual blob. The first one is tiny and will be fetched via a regular remote store, while
    /// the second one is fetched via the LFS remote store. Unfortunately, while executing a
    /// `HgIdDataStore::get`, it is possible that a fast remote store (memcache) can successfully
    /// fetch the first part but won't be able to recover the blob, causing the `get` call to
    /// return `Ok(None)`, the slower remote store (via ssh or http), will then attempt to re-fetch
    /// the pointer.
    ///
    /// This function is only intended to be implemented by the LFS store, all the others should
    /// use the default implementation below. A non-empty input slice will always return a
    /// non-empty Vec.
    ///
    /// A much better long term solution is to have `HgIdDataStore::get` return a union of a blob
    /// and a `StoreKey`, the `StoreKey` case will be chained through all the stores as a way to
    /// narrow the search space. For the case described above, the memcache store after fetching
    /// the pointer will return `Ok(Partial(StoreKey))`.
    fn translate_lfs_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }

    /// Test whether this `Store` contains a specific key.
    fn contains(&self, key: &StoreKey) -> Result<bool> {
        Ok(self.get_missing(&[key.clone()])?.is_empty())
    }
}

/// All the types that can `Deref` into a `Store` implements `Store`.
impl<T: LocalStore + ?Sized, U: Deref<Target = T> + Send + Sync> LocalStore for U {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        T::get_missing(self, keys)
    }
}
