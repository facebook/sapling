/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use smallvec::SmallVec;

#[derive(Clone, Debug)]
pub struct TrieMap<V> {
    pub value: Option<Box<V>>,
    pub edges: BTreeMap<u8, Self>,
}

impl<V> Default for TrieMap<V> {
    fn default() -> Self {
        Self {
            value: Default::default(),
            edges: Default::default(),
        }
    }
}

impl<V: PartialEq> PartialEq for TrieMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.edges == other.edges
    }
}

impl<V> TrieMap<V> {
    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<&V> {
        let mut node = self;
        for next_byte in key.as_ref() {
            match node.edges.get(next_byte) {
                Some(child) => node = child,
                None => return None,
            }
        }
        node.value.as_ref().map(|value| value.as_ref())
    }

    pub fn insert<K: AsRef<[u8]>>(&mut self, key: K, value: V) -> Option<V> {
        let node = key.as_ref().iter().fold(self, |node, next_byte| {
            node.edges.entry(*next_byte).or_default()
        });

        node.value.replace(Box::new(value)).map(|v| *v)
    }

    pub fn expand(self) -> (Option<V>, Vec<(u8, Self)>) {
        (self.value.map(|v| *v), self.edges.into_iter().collect())
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_none() && self.edges.is_empty()
    }

    pub fn clear(&mut self) {
        self.value = None;
        self.edges.clear();
    }

    pub fn iter(&self) -> TrieMapIter<'_, V> {
        TrieMapIter {
            bytes: Default::default(),
            value: self.value.as_ref().map(|value| value.as_ref()),
            stack: vec![self.edges.iter()],
        }
    }

    pub fn keys(&self) -> TrieMapKeys<'_, V> {
        TrieMapKeys {
            bytes: Default::default(),
            has_value: self.value.is_some(),
            stack: vec![self.edges.iter()],
        }
    }

    pub fn values(&self) -> TrieMapValues<'_, V> {
        TrieMapValues {
            value: self.value.as_ref().map(|value| value.as_ref()),
            stack: vec![self.edges.iter()],
        }
    }

    /// Returns a tuple of the longest common prefix of all entries in the TrieMap,
    /// and the TrieMap with the longest common prefix removed.
    // Example input:
    //     *
    //     |
    //     a
    //     |
    //     b
    //   /   \
    //  c=1   d
    //        |
    //        e=4
    //
    // Example output:
    // longest common prefix = [a, b]
    //     *
    //   /   \
    //  c=1   d
    //        |
    //        e=4
    pub fn split_longest_common_prefix(mut self) -> (SmallVec<[u8; 24]>, Self) {
        let mut lcp: SmallVec<[u8; 24]> = Default::default();

        loop {
            if self.value.is_some() || self.edges.len() > 1 {
                return (lcp, self);
            }

            match self.edges.pop_first() {
                None => return (lcp, self),
                Some((next_byte, child)) => {
                    lcp.push(next_byte);
                    self = child;
                }
            }
        }
    }
}

impl<V> TrieMap<V>
where
    V: Default,
{
    pub fn get_or_insert_default<K: AsRef<[u8]>>(&mut self, key: K) -> &mut V {
        let node = key.as_ref().iter().fold(self, |node, next_byte| {
            node.edges
                .entry(*next_byte)
                .or_insert_with(Default::default)
        });

        node.value.get_or_insert_with(Default::default)
    }
}

/// A consuming ordered iterator over all key-value pairs of a TrieMap.
// The iterator works by keeping the state of a depth first search performed
// on the trie:
// - `bytes` contains a concatenation of all u8s from the root to the current
// node in the search.
// - `value` is the value of the current node in the search, if any.
// - `stack` contains an iterator for each level descended in the depth first
// search, to allow continuing the search after backtracking from a level.
pub struct TrieMapIntoIter<V> {
    bytes: SmallVec<[u8; 24]>,
    value: Option<Box<V>>,
    stack: Vec<std::collections::btree_map::IntoIter<u8, TrieMap<V>>>,
}

impl<V> IntoIterator for TrieMap<V> {
    type Item = (SmallVec<[u8; 24]>, V);
    type IntoIter = TrieMapIntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        TrieMapIntoIter {
            bytes: Default::default(),
            value: self.value,
            stack: vec![self.edges.into_iter()],
        }
    }
}

impl<V> Iterator for TrieMapIntoIter<V> {
    type Item = (SmallVec<[u8; 24]>, V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(value) = self.value.take() {
                return Some((SmallVec::from_slice(self.bytes.as_ref()), *value));
            }

            match self.stack.last_mut() {
                None => return None,
                Some(iter) => match iter.next() {
                    None => {
                        self.bytes.pop();
                        self.stack.pop();
                    }
                    Some((next_byte, child)) => {
                        self.bytes.push(next_byte);
                        self.value = child.value;
                        self.stack.push(child.edges.into_iter());
                    }
                },
            };
        }
    }
}

/// A non-consuming ordered iterator over all key-value pairs of a TrieMap.
/// Note: the iterator has to allocate memory for the keys as they are
/// defined implicitly.
// Same as TrieMapIntoIter except that it stores a reference to the
// current node's value in `value` instead of owning it.
pub struct TrieMapIter<'a, V> {
    bytes: SmallVec<[u8; 24]>,
    value: Option<&'a V>,
    stack: Vec<std::collections::btree_map::Iter<'a, u8, TrieMap<V>>>,
}

impl<'a, V> Iterator for TrieMapIter<'a, V> {
    type Item = (SmallVec<[u8; 24]>, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(value) = self.value.take() {
                return Some((self.bytes.clone(), value));
            }

            match self.stack.last_mut() {
                None => return None,
                Some(iter) => match iter.next() {
                    None => {
                        self.bytes.pop();
                        self.stack.pop();
                    }
                    Some((next_byte, child)) => {
                        self.bytes.push(*next_byte);
                        self.value = child.value.as_ref().map(|value| value.as_ref());
                        self.stack.push(child.edges.iter());
                    }
                },
            };
        }
    }
}

/// A non-consuming ordered iterator over all keys of a TrieMap.
/// Note: the iterator has to allocate memory for the keys as they
/// are defined implicitly.
// Same as TrieMapIter except that it only needs to know whether
// the current node has a value or not without requiring a reference
// to it.
pub struct TrieMapKeys<'a, V> {
    bytes: SmallVec<[u8; 24]>,
    has_value: bool,
    stack: Vec<std::collections::btree_map::Iter<'a, u8, TrieMap<V>>>,
}

impl<'a, V> Iterator for TrieMapKeys<'a, V> {
    type Item = SmallVec<[u8; 24]>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.has_value {
                self.has_value = false;
                return Some(self.bytes.clone());
            }

            match self.stack.last_mut() {
                None => return None,
                Some(iter) => match iter.next() {
                    None => {
                        self.bytes.pop();
                        self.stack.pop();
                    }
                    Some((next_byte, child)) => {
                        self.bytes.push(*next_byte);
                        self.has_value = child.value.is_some();
                        self.stack.push(child.edges.iter());
                    }
                },
            };
        }
    }
}

/// A non-consuming ordered iterator over all values of a TrieMap.
// Same as TrieMapIter except that it doesn't need to store
// the concatenated bytes of the path from the root.
pub struct TrieMapValues<'a, V> {
    value: Option<&'a V>,
    stack: Vec<std::collections::btree_map::Iter<'a, u8, TrieMap<V>>>,
}

impl<'a, V> Iterator for TrieMapValues<'a, V> {
    type Item = &'a V;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(value) = self.value.take() {
                return Some(value);
            }

            match self.stack.last_mut() {
                None => return None,
                Some(iter) => match iter.next() {
                    None => {
                        self.stack.pop();
                    }
                    Some((_next_byte, child)) => {
                        self.value = child.value.as_ref().map(|value| value.as_ref());
                        self.stack.push(child.edges.iter());
                    }
                },
            };
        }
    }
}

impl<K: AsRef<[u8]>, V> Extend<(K, V)> for TrieMap<V> {
    fn extend<T: IntoIterator<Item = (K, V)>>(&mut self, iter: T) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

impl<K: AsRef<[u8]>, V> FromIterator<(K, V)> for TrieMap<V> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let mut trie_map: Self = Default::default();
        trie_map.extend(iter);
        trie_map
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use itertools::Itertools;

    use super::*;

    #[test]
    fn trie_map() -> Result<()> {
        let mut trie_map: TrieMap<i32> = Default::default();

        assert_eq!(trie_map.insert("abcde", 1), None);
        assert_eq!(trie_map.insert("abcdf", 2), None);
        assert_eq!(trie_map.insert("bcdf", 3), None);
        assert_eq!(trie_map.insert("abcde", 4), Some(1));

        // trie_map:
        //      *
        //     / \
        //    a   b
        //    |   |
        //    b   c
        //    |   |
        //    c   d
        //    |   |
        //    d   f=3
        //   / \
        // e=4 f=2
        assert_eq!(
            trie_map.clone().into_iter().collect::<Vec<_>>(),
            vec![
                (SmallVec::from_slice("abcde".as_bytes()), 4),
                (SmallVec::from_slice("abcdf".as_bytes()), 2),
                (SmallVec::from_slice("bcdf".as_bytes()), 3),
            ]
        );

        assert_eq!(
            trie_map
                .iter()
                .map(|(key, value)| (key, *value))
                .collect::<Vec<_>>(),
            vec![
                (SmallVec::from_slice("abcde".as_bytes()), 4),
                (SmallVec::from_slice("abcdf".as_bytes()), 2),
                (SmallVec::from_slice("bcdf".as_bytes()), 3),
            ]
        );

        assert_eq!(
            trie_map.keys().collect::<Vec<_>>(),
            vec![
                SmallVec::<[u8; 24]>::from_slice("abcde".as_bytes()),
                SmallVec::<[u8; 24]>::from_slice("abcdf".as_bytes()),
                SmallVec::<[u8; 24]>::from_slice("bcdf".as_bytes()),
            ]
        );

        assert_eq!(
            trie_map.values().copied().collect::<Vec<_>>(),
            vec![4, 2, 3]
        );

        assert_eq!(
            trie_map.clone().split_longest_common_prefix(),
            (
                SmallVec::<[u8; 24]>::new(),
                vec![("abcde", 4), ("abcdf", 2), ("bcdf", 3),]
                    .into_iter()
                    .collect()
            )
        );

        assert_eq!(trie_map.get("abcde"), Some(&4));
        assert_eq!(trie_map.get("abcdd"), None);
        assert_eq!(trie_map.get("bcdf"), Some(&3));
        assert_eq!(trie_map.get("zzzz"), None);

        let value = trie_map.get_or_insert_default("abcde");
        assert_eq!(value, &4);
        *value = 5;
        assert_eq!(trie_map.get("abcde"), Some(&5));

        let value = trie_map.get_or_insert_default("zzzz");
        assert_eq!(value, &0);
        *value = 6;
        assert_eq!(trie_map.get("zzzz"), Some(&6));

        // trie_map after modifications:
        //       *
        //     / | \
        //    a  b  z
        //    |  |  |
        //    b  c  z
        //    |  |  |
        //    c  d  z=6
        //    |  |
        //    d  f=3
        //   / \
        // e=5 f=2
        let (root_value, children) = trie_map.expand();

        assert_eq!(root_value, None);
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].0, b'a');
        assert_eq!(children[1].0, b'b');
        assert_eq!(children[2].0, b'z');

        let (a_child, b_child, mut z_child) = children
            .into_iter()
            .map(|(_byte, child)| child)
            .collect_tuple()
            .unwrap();

        // a_child:
        //    *
        //    |
        //    b
        //    |
        //    c
        //    |
        //    d
        //   / \
        //  e=5 f=2
        assert_eq!(
            a_child.clone().into_iter().collect::<Vec<_>>(),
            vec![
                (SmallVec::from_slice("bcde".as_bytes()), 5),
                (SmallVec::from_slice("bcdf".as_bytes()), 2),
            ]
        );

        assert_eq!(
            a_child.clone().split_longest_common_prefix(),
            (
                SmallVec::<[u8; 24]>::from_slice("bcd".as_bytes()),
                vec![("e", 5), ("f", 2)].into_iter().collect()
            )
        );

        // b_child:
        //    *
        //    |
        //    c
        //    |
        //    d
        //    |
        //    f=3
        assert_eq!(
            b_child.clone().into_iter().collect::<Vec<_>>(),
            vec![(SmallVec::from_slice("cdf".as_bytes()), 3)]
        );

        assert_eq!(
            b_child.clone().split_longest_common_prefix(),
            (
                SmallVec::<[u8; 24]>::from_slice("cdf".as_bytes()),
                vec![("", 3)].into_iter().collect()
            )
        );

        // z_child:
        //    *
        //    |
        //    z
        //    |
        //    z
        //    |
        //    z=6
        assert_eq!(
            z_child.clone().into_iter().collect::<Vec<_>>(),
            vec![(SmallVec::from_slice("zzz".as_bytes()), 6)]
        );

        assert_eq!(
            z_child.clone().split_longest_common_prefix(),
            (
                SmallVec::<[u8; 24]>::from_slice("zzz".as_bytes()),
                vec![("", 6)].into_iter().collect()
            )
        );

        z_child.insert("zzx", 10);
        // z_child after modification:
        //    *
        //    |
        //    z
        //    |
        //    z
        //   / \
        // x=10 z=6

        assert_eq!(
            z_child.clone().split_longest_common_prefix(),
            (
                SmallVec::<[u8; 24]>::from_slice("zz".as_bytes()),
                vec![("x", 10), ("z", 6)].into_iter().collect()
            )
        );

        Ok(())
    }
}
