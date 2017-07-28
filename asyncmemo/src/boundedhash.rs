// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::hash::Hash;
use std::marker::PhantomData;

use linked_hash_map::{self, LinkedHashMap};
use weight::Weight;

#[derive(Debug, Clone)]
pub struct BoundedHash<K, V>
where
    K: Eq + Hash,
{
    hash: LinkedHashMap<K, V>,

    entrylimit: usize, // max number of entries
    weightlimit: usize, // max weight of entries

    keysizes: usize, // sum of key weights
    entrysizes: usize, // sum of (completed) entry weights
}

pub struct Entry<'a, K, V>
where
    K: 'a + Eq + Hash,
    V: 'a,
{
    hash: *mut BoundedHash<K, V>, // promise to just touch sizes
    entry: linked_hash_map::OccupiedEntry<'a, K, V>,
    _phantom: PhantomData<&'a mut BoundedHash<K, V>>, // lifetime for pointer
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

    pub fn weightlimit(&self) -> usize {
        self.weightlimit
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
    pub fn insert(&mut self, k: K, v: V) -> Result<(), (K, V)> {
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

        if let Some(oldv) = self.hash.insert(k, v) {
            self.entrysizes -= oldv.get_weight();
        }

        Ok(())
    }

    #[cfg(test)]
    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.hash.get(key)
    }

    /// Similar to other `Entry` variants, but only for occuped entries
    pub fn entry(&mut self, key: K) -> Option<Entry<K, V>> {
        let ptr = self as *mut _;
        if let linked_hash_map::Entry::Occupied(occ) = self.hash.entry(key) {
            Some(Entry {
                hash: ptr,
                entry: occ,
                _phantom: PhantomData,
            })
        } else {
            None
        }
    }
}

impl<'a, K, V> Entry<'a, K, V>
where
    K: 'a + Eq + Hash + Weight,
    V: 'a + Weight,
{
    /// Get a reference to the entry
    pub fn get(&self) -> &V {
        self.entry.get()
    }

    /// Get a mutable reference. The caller must be careful not to change the weight
    /// of the entry - it will not be accounted for (use `update()` to change an entry along
    /// with its weight).
    pub fn get_mut(&mut self) -> &mut V {
        self.entry.get_mut()
    }

    /// Returns true if a new value will fit into the overall hash's weight limit
    pub fn may_fit(&self, new: &V) -> bool {
        let oldw = self.get().get_weight();
        let neww = new.get_weight();

        unsafe {
            let weight = (&*self.hash).total_weight();
            weight + neww - oldw < (*self.hash).weightlimit
        }
    }

    /// Update an entry, including adjusting for a weight change.
    pub fn update(&mut self, new: V) {
        // XXX can't trim here, so may allow out of bounds. Use may_fit() to check first.
        let neww = new.get_weight();
        let old = self.entry.insert(new);

        unsafe {
            (*self.hash).entrysizes += neww - old.get_weight();
        }
    }

    /// Remove the entry.
    pub fn remove(self) {
        unsafe {
            (*self.hash).keysizes -= self.entry.key().get_weight();
            (*self.hash).entrysizes -= self.get().get_weight();
        }
        let _ = self.entry.remove();
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
    fn entry() {
        let mut c = BoundedHash::new(10, 1000);

        let ok = c.insert(Weighted("hello", 10), Weighted("world", 100))
            .is_ok();

        assert!(ok, "insert failed");
        assert_eq!(c.total_weight(), 10 + 100);
        assert_eq!(c.len(), 1);

        {
            let mut ent = c.entry(Weighted("hello", 10)).expect("entry failed");
            assert_eq!(ent.get(), &Weighted("world", 100));
            ent.update(Weighted("jupiter", 500));
        }

        assert_eq!(c.total_weight(), 10 + 500);
        assert_eq!(c.len(), 1);

        {
            let v = c.get(&Weighted("hello", 10)).expect("get failed");
            assert_eq!(v, &Weighted("jupiter", 500));
        }

        {
            let ent = c.entry(Weighted("hello", 10)).expect("entry failed");
            assert_eq!(ent.get(), &Weighted("jupiter", 500));
            ent.remove();
        }

        assert_eq!(c.total_weight(), 0);
        assert_eq!(c.len(), 0);
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
