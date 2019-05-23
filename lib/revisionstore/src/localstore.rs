// Copyright 2019 Facebook, Inc.
//! Common method shared between all mercurial stores

use std::{ops::Deref, path::Path};

use failure::Fallible;

use types::Key;

pub trait LocalStore {
    /// Builds a Store from a filepath. The default implementation panics.
    fn from_path(_path: &Path) -> Fallible<Self>
    where
        Self: Sized,
    {
        unimplemented!("Can't build a Store");
    }

    /// Returns all the keys that aren't present in this `Store`.
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>>;

    /// Test whether this `Store` contains a specific key.
    fn contains(&self, key: &Key) -> Fallible<bool> {
        Ok(self.get_missing(&[key.clone()])?.is_empty())
    }
}

/// All the types that can `Deref` into a `Store` implements `Store`.
impl<T: LocalStore + ?Sized, U: Deref<Target = T>> LocalStore for U {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        T::get_missing(self, keys)
    }
}
