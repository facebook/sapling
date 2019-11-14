/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Union store

use std::{slice::Iter, vec::IntoIter};

use failure::Fallible;

use types::Key;

use crate::{localstore::LocalStore, repack::ToKeys};

pub struct UnionStore<T> {
    stores: Vec<T>,
}

impl<T> UnionStore<T> {
    pub fn new() -> UnionStore<T> {
        UnionStore { stores: Vec::new() }
    }

    pub fn add(&mut self, item: T) {
        self.stores.push(item)
    }
}

impl<T: LocalStore> LocalStore for UnionStore<T> {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        let initial_keys = Ok(keys.iter().cloned().collect());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

impl<T> IntoIterator for UnionStore<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.stores.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a UnionStore<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.stores.iter()
    }
}

impl<T: ToKeys> ToKeys for UnionStore<T> {
    fn to_keys(&self) -> Vec<Fallible<Key>> {
        self.into_iter()
            .map(|store| store.to_keys())
            .flatten()
            .collect()
    }
}
