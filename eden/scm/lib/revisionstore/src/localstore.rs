/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common method shared between all mercurial stores

use std::{ops::Deref, path::Path};

use failure::Fallible as Result;

use types::Key;

pub trait LocalStore: Send + Sync {
    /// Builds a Store from a filepath. The default implementation panics.
    fn from_path(_path: &Path) -> Result<Self>
    where
        Self: Sized,
    {
        unimplemented!("Can't build a Store");
    }

    /// Returns all the keys that aren't present in this `Store`.
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>>;

    /// Test whether this `Store` contains a specific key.
    fn contains(&self, key: &Key) -> Result<bool> {
        Ok(self.get_missing(&[key.clone()])?.is_empty())
    }
}

/// All the types that can `Deref` into a `Store` implements `Store`.
impl<T: LocalStore + ?Sized, U: Deref<Target = T> + Send + Sync> LocalStore for U {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        T::get_missing(self, keys)
    }
}
