/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::hash::Hash;

/// A map implementation that counts the number of times a key is inserted.
///
/// The interface is roughly based on std::collections::HashMap, but is changed
/// and extended to accommodate the counter use case.
///
/// NOTE: This is not a general purpose map implementation. It is designed
/// specifically for cases where each key always maps to the same value.
pub struct CountedMap<K, V> {
    inner: HashMap<K, (V, usize)>,
}

impl<K, V> CountedMap<K, V>
where
    K: Eq + Hash,
    V: Clone,
{
    /// Inserts a key into the map, or updates the count if the key already exists.
    pub fn insert(&mut self, key: K, value: V) {
        self.inner
            .entry(key)
            .and_modify(|(_, count)| *count += 1)
            .or_insert((value, 1));
    }

    /// Returns a reference to the value corresponding to the key.
    #[allow(dead_code)]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.inner.get(key).map(|(v, _)| v)
    }

    /// Removes a key from the map, or decrements the count if the key already exists.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some((value, count)) = self.inner.get_mut(key) {
            if *count > 1 {
                *count -= 1;
                return Some(value.clone());
            } else if let Some((v, _c)) = self.inner.remove(key) {
                // _c == 1
                return Some(v);
            }
        }
        None
    }

    /// Return true if the map contains the key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }
}

impl<K, V> Default for CountedMap<K, V>
where
    K: Eq + Hash,
{
    fn default() -> CountedMap<K, V> {
        CountedMap {
            inner: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CountedMap;

    #[test]
    fn test_inserts() {
        let mut map = CountedMap::default();
        map.insert(1, "a");
        map.insert(2, "b");
        map.insert(3, "c");
        map.insert(3, "c");
        map.insert(2, "b");

        assert_eq!(map.get(&1), Some(&"a"));
        assert_eq!(map.get(&2), Some(&"b"));
        assert_eq!(map.get(&3), Some(&"c"));
    }

    #[test]
    fn test_removes() {
        let mut map = CountedMap::default();
        map.insert(1, "a");
        map.insert(2, "b");
        map.insert(2, "b");

        assert_eq!(map.remove(&1), Some("a"));
        assert_eq!(map.remove(&1), None);

        assert_eq!(map.remove(&2), Some("b"));
        assert_eq!(map.remove(&2), Some("b"));
        assert_eq!(map.remove(&2), None);
    }

    #[test]
    fn test_contains_key() {
        let mut map = CountedMap::default();
        map.insert(1, "a");
        map.insert(2, "b");
        map.insert(2, "b");

        assert!(map.contains_key(&1));
        assert_eq!(map.remove(&1), Some("a"));
        assert!(!map.contains_key(&1));

        assert!(map.contains_key(&2));
        assert_eq!(map.remove(&2), Some("b"));
        assert!(map.contains_key(&2));
        assert_eq!(map.remove(&2), Some("b"));
        assert!(!map.contains_key(&2));
    }
}
