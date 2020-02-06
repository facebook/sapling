/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::mem;
use std::{u64, usize};

use asyncmemo::Weight;
use heapsize_derive::HeapSizeOf;
use serde_derive::Serialize;

/// Generation number
///
/// The generation number for a changeset is defined as the max of the changeset's parents'
/// generation number plus 1; if there are no parents then it's 1.
#[derive(
    Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, HeapSizeOf, Serialize
)]
pub struct Generation(u64);

impl Weight for Generation {
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>()
    }
}

impl Generation {
    /// Creates new generation number
    pub fn new(gen: u64) -> Self {
        Generation(gen)
    }

    /// Create a maximum possible generation number
    pub fn max_gen() -> Self {
        Generation(u64::MAX)
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    /// The difference from this generation to the other as the difference in their
    /// generation numbers.
    /// If this Generation is smaller than the other, return None.
    pub fn difference_from(&self, other: Generation) -> Option<u64> {
        let Generation(self_gen) = self;
        let Generation(other_gen) = other;
        self_gen.checked_sub(other_gen)
    }
}
