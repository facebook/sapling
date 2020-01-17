/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{BinaryHeap, HashSet};
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct UniqueHeap<T>
where
    T: Clone + Ord + Hash + Eq,
{
    sorted_vals: BinaryHeap<T>,
    unique_vals: HashSet<T>,
}

impl<T> UniqueHeap<T>
where
    T: Clone + Ord + Hash + Eq + Clone,
{
    pub fn new() -> Self {
        UniqueHeap {
            sorted_vals: BinaryHeap::new(),
            unique_vals: HashSet::new(),
        }
    }

    pub fn push(&mut self, val: T) {
        if !self.unique_vals.contains(&val) {
            self.unique_vals.insert(val.clone());
            self.sorted_vals.push(val.clone());
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        if let Some(v) = self.sorted_vals.pop() {
            self.unique_vals.remove(&v);
            Some(v)
        } else {
            None
        }
    }

    pub fn peek(&self) -> Option<&T> {
        self.sorted_vals.peek()
    }
}
