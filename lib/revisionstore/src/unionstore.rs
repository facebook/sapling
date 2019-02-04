// Copyright Facebook, Inc. 2018
// Union store

use std::vec::IntoIter;

pub struct UnionStore<T> {
    stores: Vec<T>,
}

pub struct UnionStoreIterator<T>(IntoIter<T>);

impl<T> UnionStore<T> {
    pub fn new() -> UnionStore<T> {
        UnionStore { stores: Vec::new() }
    }

    pub fn add(&mut self, item: T)
    where
        T: Clone,
    {
        self.stores.push(item)
    }
}

impl<'a, T> IntoIterator for &'a UnionStore<T>
where
    T: Clone,
{
    type Item = T;
    type IntoIter = UnionStoreIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        UnionStoreIterator(self.stores.clone().into_iter())
    }
}

impl<T> Iterator for UnionStoreIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}
