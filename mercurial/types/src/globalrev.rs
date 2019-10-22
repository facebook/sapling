/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// Changeset globalrev.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Globalrev(u64);

impl Globalrev {
    #[inline]
    pub const fn new(rev: u64) -> Self {
        Self(rev)
    }

    #[inline]
    pub fn id(&self) -> u64 {
        self.0
    }
}
