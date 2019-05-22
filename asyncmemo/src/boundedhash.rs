// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::hash::Hash;

use crate::weight::Weight;
use linked_hash_map::LinkedHashMap;

#[derive(Debug, Clone)]
pub struct BoundedHash<K, V>
where
    K: Eq + Hash,
{
    hash: LinkedHashMap<K, V>,

    entrylimit: usize,  // max number of entries
    weightlimit: usize, // max weight of entries

    entrysizes: usize, // sum of (completed) entry weights
}

impl<K, V> BoundedHash<K, V>
where
    K: Eq + Hash,
    V: Weight,
{
    pub fn new(entrylimit: usize, weightlimit: usize) -> Self {
        BoundedHash {
            hash: LinkedHashMap::new(),
            entrysizes: 0,
            entrylimit,
            weightlimit,
        }
    }

    pub fn total_weight(&self) -> usize {
        self.entrysizes
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.hash.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.hash.is_empty()
    }

    fn remove_one(&mut self, v: &V) {
        self.entrysizes -= v.get_weight();
    }

    /// Trim an entry with LRU policy
    fn trim_one(&mut self) -> bool {
        match self.hash.pop_front() {
            Some((_k, v)) => {
                self.remove_one(&v);
                true
            }
            None => false,
        }
    }

    /// Trim enough entries to make room for `additional` new ones.
    pub fn trim_entries(&mut self, additional: usize) -> bool {
        if additional > self.entrylimit {
            return false;
        }

        let limit = self.entrylimit - additional;

        while self.hash.len() > limit {
            if !self.trim_one() {
                break;
            }
        }

        true
    }

    /// Trim enough weight to make room for `additional` new weight.
    pub fn trim_weight(&mut self, additional: usize) -> bool {
        if additional > self.weightlimit {
            return false;
        }

        let limit = self.weightlimit - additional;

        while self.total_weight() > limit {
            if !self.trim_one() {
                break;
            }
        }

        true
    }

    /// Trim back to desired limits
    pub fn trim(&mut self) {
        self.trim_entries(0);
        self.trim_weight(0);
    }

    pub fn clear(&mut self) {
        self.hash.clear();
        self.entrysizes = 0;
    }

    /// Trim a specific key, returning it if it existed, after updating the weight
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.hash.remove(key).map(|v| {
            self.remove_one(&v);
            v
        })
    }

    /// Insert new entry, updating weights
    ///
    /// Insert fails if there isn't capacity for the new entry, returning the key and value.
    pub fn insert(&mut self, k: K, v: V) -> Result<Option<V>, (K, V)> {
        // Remove the key if it's already in the hash
        let oldv = self.hash.remove(&k);
        if let Some(ref removed) = oldv {
            self.entrysizes -= removed.get_weight();
        }

        if !self.trim_entries(1) {
            // seems unlikely, but anyway
            return Err((k, v));
        }

        let vw = v.get_weight();

        if !self.trim_weight(vw) {
            return Err((k, v));
        }

        self.entrysizes += vw;

        self.hash.insert(k, v);
        Ok(oldv)
    }

    #[cfg(test)]
    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.hash.get(key)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.hash.get_mut(key)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::ops::{Deref, DerefMut};

    #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
    struct Weighted<T>(T, usize);

    impl<T> Weight for Weighted<T> {
        fn get_weight(&self) -> usize {
            self.1
        }
    }

    impl<T> Deref for Weighted<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<T> DerefMut for Weighted<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    #[test]
    fn simple() {
        let mut c = BoundedHash::new(10, 1000);

        {
            let ok = c.insert("hello", Weighted("world", 100)).is_ok();

            assert!(ok, "insert failed");
            assert_eq!(c.total_weight(), 100);
            assert_eq!(c.len(), 1);
        }

        {
            let v = c.get(&"hello").expect("get failed");
            assert_eq!(v, &Weighted("world", 100));
        }

        {
            let ok = c.remove(&"hello").is_some();

            assert!(ok, "remove failed");
            assert_eq!(c.total_weight(), 0);
            assert_eq!(c.len(), 0);
        }
    }

    #[test]
    fn toobig() {
        let mut c = BoundedHash::new(10, 1000);

        let ok = c.insert("hello", Weighted("world", 100)).is_ok();

        assert!(ok, "insert failed");
        assert_eq!(c.total_weight(), 100);
        assert_eq!(c.len(), 1);

        let err = c.insert("bubble", Weighted("lead", 1001)).is_err();
        assert!(err, "insert worked?");

        assert_eq!(c.total_weight(), 100);
        assert_eq!(c.len(), 1);

        let ok = c.insert("bubble", Weighted("balloon", 880)).is_ok();
        assert!(ok, "insert failed?");

        assert_eq!(c.total_weight(), 100 + 880);
        assert_eq!(c.len(), 2);
    }
}
