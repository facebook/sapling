/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ordered map implementation using a sorted vector

use std::borrow::Borrow;
use std::collections::Bound;
use std::collections::Bound::*;
use std::mem;
use std::slice::{Iter as VecIter, IterMut as VecIterMut};

#[derive(Debug)]
pub struct VecMap<K, V> {
    vec: Vec<(K, V)>,
}

pub struct Iter<'a, K: 'a, V: 'a>(VecIter<'a, (K, V)>);

pub struct IterMut<'a, K: 'a, V: 'a>(VecIterMut<'a, (K, V)>);

impl<K, V> VecMap<K, V>
where
    K: Ord,
{
    /// Creates a new, empty VecMap.
    pub fn new() -> VecMap<K, V> {
        VecMap { vec: Vec::new() }
    }

    /// Creates a new, empty VecMap, with capacity for `capacity` entries.
    pub fn with_capacity(capacity: usize) -> VecMap<K, V> {
        VecMap {
            vec: Vec::with_capacity(capacity),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    pub fn len(&self) -> usize {
        self.vec.len()
    }

    /// Utility function to binary search for an index using the key.
    fn find_index<Q: ?Sized>(&self, q: &Q) -> Result<usize, usize>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        self.vec.binary_search_by(|e| e.0.borrow().cmp(q))
    }

    /// Inserts a key-value pair into the map.  If the key already had a value present in the
    /// map, that value is replaced and returned.  Otherwise, `None` is returned.
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        let mut v = v;
        match self.find_index(&k) {
            Ok(index) => {
                mem::swap(&mut self.vec[index].1, &mut v);
                Some(v)
            }
            Err(index) => {
                self.vec.insert(index, (k, v));
                None
            }
        }
    }

    /// Inserts a key-value pair into the map.  Fast-path for when the key is not already
    /// present and is at the end of the map.
    pub fn insert_hint_end(&mut self, k: K, v: V) -> Option<V> {
        let len = self.vec.len();
        if len == 0 || self.vec[len - 1].0 < k {
            self.vec.push((k, v));
            None
        } else {
            self.insert(k, v)
        }
    }

    /// Removes a key-value pair from the map, returning the value if the key was previously in
    /// the map.
    pub fn remove<Q: ?Sized>(&mut self, q: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        match self.find_index(q) {
            Ok(index) => {
                let (_k, v) = self.vec.remove(index);
                Some(v)
            }
            Err(_index) => None,
        }
    }

    /// Returns a reference to the value corresponding to the key.
    pub fn get<'a, Q: ?Sized>(&'a self, q: &Q) -> Option<&'a V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        match self.find_index(q) {
            Ok(index) => Some(&self.vec[index].1),
            Err(_index) => None,
        }
    }

    /// Returns a mutable reference to the value corresponding to the key.
    pub fn get_mut<'a, Q: ?Sized>(&'a mut self, q: &Q) -> Option<&'a mut V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        match self.find_index(q) {
            Ok(index) => Some(&mut self.vec[index].1),
            Err(_index) => None,
        }
    }

    // Returns an iterator over the pairs of entries in the map.
    pub fn iter(&self) -> Iter<K, V> {
        Iter(self.vec.iter())
    }

    /// Returns a mutable iterator over the pairs of entries in the map.
    pub fn iter_mut(&mut self) -> IterMut<K, V> {
        IterMut(self.vec.iter_mut())
    }

    /// Utility function for implementing `range` and `range_mut`.  Convert a range boundary for
    /// the start of a range into a slice index suitable for use in a range expression.
    fn range_index_start<Q: ?Sized>(&self, b: Bound<&Q>) -> usize
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        match b {
            Unbounded => 0,
            Included(q) => match self.find_index(q) {
                Ok(index) => index,
                Err(index) => index,
            },
            Excluded(q) => match self.find_index(q) {
                Ok(index) => index + 1,
                Err(index) => index,
            },
        }
    }

    /// Utility function for implementing `range` and `range_mut`.  Convert a range boundary for
    /// the end of a range into a slice index suitable for use in a range expression.
    fn range_index_end<Q: ?Sized>(&self, b: Bound<&Q>) -> usize
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        match b {
            Unbounded => self.vec.len(),
            Included(q) => match self.find_index(q) {
                Ok(index) => index + 1,
                Err(index) => index,
            },
            Excluded(q) => match self.find_index(q) {
                Ok(index) => index,
                Err(index) => index,
            },
        }
    }

    /// Returns an iterator over the given range of keys.
    pub fn range<Q>(&self, range: (Bound<&Q>, Bound<&Q>)) -> Iter<K, V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let start = self.range_index_start(range.0);
        let end = self.range_index_end(range.1);
        Iter(self.vec[start..end].iter())
    }

    /// Returns a mutuable iterator over the given range of keys.
    pub fn range_mut<Q>(&mut self, range: (Bound<&Q>, Bound<&Q>)) -> IterMut<K, V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let start = self.range_index_start(range.0);
        let end = self.range_index_end(range.1);
        IterMut(self.vec[start..end].iter_mut())
    }
}

// Wrap `Iter` and `IterMut` for `VecMap` types.  These implementations adapt the `next` methods,
// converting their yielded types from `Option<&(K, V)>` to `Option<(&K, &V)>`, and from
// `Option<&mut (K, V)>` to `Option<(&K, &mut V)>`.  This allows `VecMap` iterators to be used
// in the same way as other map iterators to iterate over key-value pairs, and prevents callers
// from using the mutable iterator to mutate keys.

impl<'a, K: 'a, V: 'a> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|&(ref k, ref v)| (k, v))
    }
}

impl<'a, K: 'a, V: 'a> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|&mut (ref k, ref mut v)| (k, v))
    }
}

#[cfg(test)]
mod tests {

    use crate::vecmap::{Iter, VecMap};
    use quickcheck::quickcheck;
    use std::collections::BTreeMap;
    use std::collections::Bound::*;

    #[test]
    fn insert_get_remove() {
        let mut vm = VecMap::new();
        assert_eq!(vm.insert("test1", "value1".to_string()), None);
        assert_eq!(vm.insert("test2", "value2".to_string()), None);
        assert_eq!(vm.insert_hint_end("test4", "value4".to_string()), None);
        assert_eq!(vm.insert_hint_end("test3", "value3".to_string()), None);
        assert_eq!(
            vm.insert("test1", "value1b".to_string()),
            Some("value1".to_string())
        );
        assert_eq!(vm.get(&"test1"), Some(&"value1b".to_string()));
        if let Some(v) = vm.get_mut(&"test1") {
            *v = "value1c".to_string();
        }
        assert_eq!(vm.get(&"test1"), Some(&"value1c".to_string()));
        assert_eq!(vm.remove("test2"), Some("value2".to_string()));
        assert_eq!(vm.remove("test2"), None);
        assert_eq!(vm.get(&"test2"), None);
        assert_eq!(vm.get_mut(&"never"), None);
    }

    #[test]
    fn iter() {
        let mut vm = VecMap::with_capacity(4);
        assert!(vm.is_empty());
        vm.insert(2, "value2");
        vm.insert(1, "value1");
        vm.insert(4, "value4");
        vm.insert(3, "value3");
        assert!(!vm.is_empty());
        assert_eq!(vm.len(), 4);
        {
            let mut im = vm.iter_mut();
            im.next();
            let e2 = im.next().unwrap();
            *e2.1 = "value2 - modified";
        }
        let mut i = vm.iter();
        assert_eq!(i.next(), Some((&1, &"value1")));
        assert_eq!(i.next(), Some((&2, &"value2 - modified")));
        assert_eq!(i.next(), Some((&3, &"value3")));
        assert_eq!(i.next(), Some((&4, &"value4")));
        assert_eq!(i.next(), None);
    }

    #[test]
    fn range() {
        let mut vm: VecMap<i32, i32> = VecMap::new();
        for n in 0..20 {
            vm.insert(n * 2, n * 4);
        }

        fn check_iter(mut x: Iter<i32, i32>, start: i32, end: i32) {
            let mut i = start;
            while i < end {
                assert_eq!(x.next(), Some((&i, &(i * 2))));
                i += 2;
            }
            assert_eq!(x.next(), None);
        }

        check_iter(vm.range((Unbounded, Unbounded)), 0, 39);
        check_iter(vm.range((Unbounded, Included(&2))), 0, 3);
        check_iter(vm.range((Unbounded, Excluded(&2))), 0, 1);
        check_iter(vm.range((Unbounded, Excluded(&7))), 0, 7);
        check_iter(vm.range((Unbounded, Included(&13))), 0, 13);
        check_iter(vm.range((Included(&4), Included(&13))), 4, 13);
        check_iter(vm.range((Included(&5), Included(&14))), 6, 15);
        check_iter(vm.range((Excluded(&5), Included(&20))), 6, 21);
        check_iter(vm.range((Excluded(&6), Included(&60))), 8, 39);
        check_iter(vm.range((Excluded(&-30), Unbounded)), 0, 39);
        check_iter(vm.range((Included(&-1), Unbounded)), 0, 39);

        assert_eq!(vm.get(&16), Some(&32));
        {
            let mut im = vm.range_mut((Included(&16), Excluded(&18)));
            *im.next().unwrap().1 *= 2;
            assert_eq!(im.next(), None);
        }
        assert_eq!(vm.get(&16), Some(&64));
    }

    fn vecmap_from_btreemap<K: Ord + Clone, V: Clone>(b: &BTreeMap<K, V>) -> VecMap<K, V> {
        let mut vm = VecMap::new();
        for (k, v) in b.iter() {
            vm.insert(k.clone(), v.clone());
        }
        vm
    }

    quickcheck! {
        fn like_btreemap_is_empty (b: BTreeMap<u32, u32>) -> bool {
            let vm = vecmap_from_btreemap(&b);
            vm.is_empty() == b.is_empty()
        }

        fn like_btreemap_len (b: BTreeMap<u32, u32>) -> bool {
            let vm = vecmap_from_btreemap(&b);
            vm.len() == b.len()
        }

        fn like_btreemap_iter (b: BTreeMap<u32, u32>) -> bool {
            let vm = vecmap_from_btreemap(&b);
            itertools::equal(vm.iter(), b.iter())
        }

        fn like_btreemap_range (b: BTreeMap<u32, u32>, key1: u32, key2: u32) -> bool {
            // range requires start key is not after end key.
            let (start, end) = if key1 <= key2 { (key1, key2) } else { (key2, key1) };
            let vm = vecmap_from_btreemap(&b);
            let range = (Included(&start), Excluded(&end));
            itertools::equal(vm.range(range), b.range(range))
        }
    }
}
