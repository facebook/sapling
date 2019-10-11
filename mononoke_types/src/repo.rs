/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use abomonation_derive::Abomonation;
use heapsize_derive::HeapSizeOf;
use serde_derive::Serialize;

/// Represents a repository. This ID is used throughout storage.
#[derive(
    Clone,
    Copy,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Debug,
    Hash,
    HeapSizeOf,
    Abomonation,
    Serialize
)]
pub struct RepositoryId(i32);

impl RepositoryId {
    // TODO: (rain1) T30368905 Instead of using this struct directly, have a wrapper around it that
    // only accepts a u32.
    #[inline]
    pub const fn new(id: i32) -> Self {
        RepositoryId(id)
    }

    #[inline]
    pub fn id(&self) -> i32 {
        self.0
    }

    /// Generate a prefix suitable for a blobstore.
    #[inline]
    pub fn prefix(&self) -> String {
        // Generate repo0001, repo0002, etc.
        format!("repo{:04}.", self.0)
    }
}

impl asyncmemo::Weight for RepositoryId {
    fn get_weight(&self) -> usize {
        std::mem::size_of::<RepositoryId>()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn prefix() {
        assert_eq!(RepositoryId(0).prefix().as_str(), "repo0000.");
        assert_eq!(RepositoryId(1).prefix().as_str(), "repo0001.");
        assert_eq!(RepositoryId(99).prefix().as_str(), "repo0099.");
        assert_eq!(RepositoryId(456).prefix().as_str(), "repo0456.");
        assert_eq!(RepositoryId(9999).prefix().as_str(), "repo9999.");
        assert_eq!(RepositoryId(12000).prefix().as_str(), "repo12000.");
    }
}
