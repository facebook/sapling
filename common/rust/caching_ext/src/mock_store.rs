/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Debug, PartialEq)]
pub struct MockStoreStats {
    pub sets: usize,
    pub gets: usize,
    pub hits: usize,
    pub misses: usize,
}

#[derive(Clone, Debug)]
pub struct MockStore<T> {
    data: Arc<Mutex<HashMap<String, T>>>,
    pub(crate) set_count: Arc<AtomicUsize>,
    pub(crate) get_count: Arc<AtomicUsize>,
    pub(crate) hit_count: Arc<AtomicUsize>,
    pub(crate) miss_count: Arc<AtomicUsize>,
}

impl<T> MockStore<T> {
    pub(crate) fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            set_count: Arc::new(AtomicUsize::new(0)),
            get_count: Arc::new(AtomicUsize::new(0)),
            hit_count: Arc::new(AtomicUsize::new(0)),
            miss_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn stats(&self) -> MockStoreStats {
        MockStoreStats {
            sets: self.set_count.load(Ordering::SeqCst),
            gets: self.get_count.load(Ordering::SeqCst),
            misses: self.miss_count.load(Ordering::SeqCst),
            hits: self.hit_count.load(Ordering::SeqCst),
        }
    }
}

impl<T: Clone> MockStore<T> {
    pub fn get(&self, key: &String) -> Option<T> {
        self.get_count.fetch_add(1, Ordering::SeqCst);
        let value = self.data.lock().expect("poisoned lock").get(key).cloned();
        match &value {
            Some(..) => self.hit_count.fetch_add(1, Ordering::SeqCst),
            None => self.miss_count.fetch_add(1, Ordering::SeqCst),
        };
        value
    }

    pub fn set(&self, key: &String, value: &T) {
        self.set_count.fetch_add(1, Ordering::SeqCst);
        self.data
            .lock()
            .expect("poisoned lock")
            .insert(key.clone(), value.clone());
    }

    #[cfg(test)]
    pub(crate) fn data(&self) -> HashMap<String, T> {
        self.data.lock().expect("poisoned lock").clone()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_counts() {
        let store = MockStore::new();
        assert_eq!(
            store.stats(),
            MockStoreStats {
                sets: 0,
                gets: 0,
                misses: 0,
                hits: 0
            }
        );

        store.set(&"foo".to_string(), &());
        assert_eq!(
            store.stats(),
            MockStoreStats {
                sets: 1,
                gets: 0,
                misses: 0,
                hits: 0
            }
        );

        store.get(&"foo".to_string());
        assert_eq!(
            store.stats(),
            MockStoreStats {
                sets: 1,
                gets: 1,
                misses: 0,
                hits: 1
            }
        );

        store.get(&"bar".to_string());
        assert_eq!(
            store.stats(),
            MockStoreStats {
                sets: 1,
                gets: 2,
                misses: 1,
                hits: 1
            }
        );
    }
}
