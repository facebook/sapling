/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings, missing_docs, clippy::all, intra_doc_link_resolution_failure)]

//! Deduplicate items by replacing them with an index into an array.

use std::borrow::{Borrow, Cow, ToOwned};
use std::collections::HashMap;
use std::hash::Hash;

/// A `DedupMap` accepts items and assigns unique values to different
/// indices in a deduplicated vector.  Items can be retrieved using
/// these indices, and the map can be frozen into a vector for efficient
/// storage.
pub struct DedupMap<T> {
    items: Vec<T>,
    indexes: HashMap<T, usize>,
}

impl<T> Default for DedupMap<T>
where
    T: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self {
            items: Vec::new(),
            indexes: HashMap::new(),
        }
    }
}

impl<T> DedupMap<T>
where
    T: Eq + Hash + Clone,
{
    /// Create a new, empty `DedupMap`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert an item into the `DedupMap`.  If the item has already
    /// been added to the map, then its existing index will be returned.
    /// Otherwise, a new index will be returned.
    pub fn insert<'a, B, N>(&mut self, value: N) -> usize
    where
        N: Into<Cow<'a, B>>,
        B: ToOwned<Owned = T> + Eq + Hash + ?Sized + 'a,
        T: Borrow<B>,
    {
        let value = value.into();
        if let Some(&index) = self.indexes.get(value.as_ref()) {
            index
        } else {
            let index = self.items.len();
            let value = value.into_owned();
            self.indexes.insert(value.clone(), index);
            self.items.push(value);
            index
        }
    }

    /// Look up an item by the index previously returned by `insert`.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index)
    }

    /// Freeze the `DedupMap` into a vector of the contained item.
    /// This can be used to look-up the indexes previously returned
    /// by `insert`.
    pub fn into_items(self) -> Vec<T> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut dedup_map: DedupMap<String> = DedupMap::new();

        // Can insert items by reference (they will be cloned if necessary).
        let item1 = dedup_map.insert("hello");
        let item2 = dedup_map.insert("rust");

        // Can also insert owned items.
        let item3 = dedup_map.insert(String::from("test"));

        // Can insert duplicate items and will get the same index.
        let item4 = dedup_map.insert("hello");
        assert_eq!(item1, item4);

        // Can read items from the dedup map.
        assert_eq!(dedup_map.get(item1), Some(&String::from("hello")));
        assert_eq!(dedup_map.get(item2), Some(&String::from("rust")));
        assert_eq!(dedup_map.get(item3), Some(&String::from("test")));

        // Can freeez the dedup map into a vector and read from that.
        let dedup_vec = dedup_map.into_items();
        assert_eq!(dedup_vec.get(item1), Some(&String::from("hello")));
        assert_eq!(dedup_vec.get(item2), Some(&String::from("rust")));
        assert_eq!(dedup_vec.get(item3), Some(&String::from("test")));
    }
}
