/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Union store

use std::slice::Iter;
use std::vec::IntoIter;

use anyhow::Result;
use types::Key;

use crate::ToKeys;
use crate::localstore::LocalStore;
use crate::types::StoreKey;

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
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let initial_keys = Ok(keys.to_vec());
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
    fn to_keys(&self) -> Vec<Result<Key>> {
        self.into_iter().flat_map(|store| store.to_keys()).collect()
    }
}
