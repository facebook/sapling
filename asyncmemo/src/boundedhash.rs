// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::hash::Hash;

use linked_hash_map::LinkedHashMap;
use weight::Weight;

#[derive(Debug, Clone)]
pub struct BoundedHash<K, V>
where
    K: Eq + Hash,
{
    hash: LinkedHashMap<K, V>,

    entrylimit: usize,  // max number of entries
    weightlimit: usize, // max weight of entries

    keysizes: usize,   // sum of key weights
    entrysizes: usize, // sum of (completed) entry weights
}

impl<K, V> BoundedHash<K, V>
where
    K: Eq + Hash + Weight,
    V: Weight,
{
    pub fn new(entrylimit: usize, weightlimit: usize) -> Self {
        BoundedHash {
            hash: LinkedHashMap::new(),
            entrysizes: 0,
            keysizes: 0,
            entrylimit,
            weightlimit,
        }
    }

    pub fn total_weight(&self) -> usize {
        self.keysizes + self.entrysizes
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.hash.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.hash.is_empty()
    }

    fn remove_one(&mut self, k: &K, v: &V) {
        self.keysizes -= k.get_weight();
        self.entrysizes -= v.get_weight();
    }

    /// Trim an entry with LRU policy
    fn trim_one(&mut self) -> bool {
        match self.hash.pop_front() {
            Some((k, v)) => {
                self.remove_one(&k, &v);
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
        self.keysizes = 0;
    }

    /// Trim a specific key, returning it if it existed, after updating the weight
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.hash.remove(key).map(|v| {
            self.remove_one(key, &v);
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
            // XXX Debug code to try and figure out T27701455.
            assert!(
                self.keysizes >= k.get_weight(),
                "ASSERTION FAILED: keysizes: {}, weight: {}",
                self.keysizes,
                k.get_weight(),
            );
            self.keysizes -= k.get_weight();
            self.entrysizes -= removed.get_weight();
        }

        if !self.trim_entries(1) {
            // seems unlikely, but anyway
            return Err((k, v));
        }

        let kw = k.get_weight();
        let vw = v.get_weight();

        if !self.trim_weight(kw + vw) {
            return Err((k, v));
        }

        self.keysizes += kw;
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
            let ok = c.insert(Weighted("hello", 10), Weighted("world", 100))
                .is_ok();

            assert!(ok, "insert failed");
            assert_eq!(c.total_weight(), 10 + 100);
            assert_eq!(c.len(), 1);
        }

        {
            let v = c.get(&Weighted("hello", 10)).expect("get failed");
            assert_eq!(v, &Weighted("world", 100));
        }

        {
            let ok = c.remove(&Weighted("hello", 10)).is_some();

            assert!(ok, "remove failed");
            assert_eq!(c.total_weight(), 0);
            assert_eq!(c.len(), 0);
        }
    }

    #[test]
    fn toobig() {
        let mut c = BoundedHash::new(10, 1000);

        let ok = c.insert(Weighted("hello", 10), Weighted("world", 100))
            .is_ok();

        assert!(ok, "insert failed");
        assert_eq!(c.total_weight(), 10 + 100);
        assert_eq!(c.len(), 1);

        let err = c.insert(Weighted("bubble", 10), Weighted("lead", 1000))
            .is_err();
        assert!(err, "insert worked?");

        assert_eq!(c.total_weight(), 10 + 100);
        assert_eq!(c.len(), 1);

        let ok = c.insert(Weighted("bubble", 10), Weighted("balloon", 880))
            .is_ok();
        assert!(ok, "insert failed?");

        assert_eq!(c.total_weight(), 10 + 100 + 10 + 880);
        assert_eq!(c.len(), 2);
    }
}
