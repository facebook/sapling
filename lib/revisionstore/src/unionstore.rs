// Copyright Facebook, Inc. 2018
// Union store

use std::slice::Iter;
use std::vec::IntoIter;

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
