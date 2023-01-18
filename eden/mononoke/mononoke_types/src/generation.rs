/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::u64;

use abomonation_derive::Abomonation;
use serde_derive::Serialize;

/// Generation number
///
/// The generation number for a changeset is defined as the max of the changeset's parents'
/// generation number plus 1; if there are no parents then it's 1.
#[derive(
    Abomonation,
    Debug,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize
)]
pub struct Generation(u64);

pub const FIRST_GENERATION: Generation = Generation(1);

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

    /// Subtracts value from generation number, returns None if the result is not a valid
    /// generation number.
    pub fn checked_sub(&self, value: u64) -> Option<Generation> {
        let Generation(self_gen) = self;
        let res = self_gen.checked_sub(value);
        if res == Some(0) {
            return None;
        }
        res.map(Generation::new)
    }

    /// Adds a value to generation number
    pub fn add(&self, value: u64) -> Generation {
        let Generation(self_gen) = self;
        Generation::new(self_gen + value)
    }
}
