// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{usize, u64};
use std::mem;

use asyncmemo::Weight;
/// Generation number
///
/// The generation number for a changeset is defined as the max of the changeset's parents'
/// generation number plus 1; if there are no parents then it's 1.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, HeapSizeOf)]
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
}
