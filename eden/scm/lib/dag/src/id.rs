/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # id
//!
//! Defines types around [`Id`].

use std::fmt;
use std::ops;

/// An [`Id`] representing a node in the graph.
/// [`Id`]s are topologically sorted.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u64);

/// An integer that separates distinct groups of [`Id`]s.
///
/// This can be seen as a way to pre-allocate consecutive integers
/// for one group to make segments less fragmented.
///
/// `(Group, Id)` are also topologically sorted.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Group(pub(crate) usize);

impl Group {
    /// The "master" group. `ancestors(master)`.
    /// - Expected to have most of the commits in a repo.
    /// - Expected to be free from fragmentation. In other words,
    ///   `ancestors(master)` can be represented in a single Span.
    pub const MASTER: Self = Self(0);

    /// The "non-master" group.
    /// - Anything not in `ancestors(master)`. For example, public release
    ///   branches, local feature branches.
    /// - Expected to have multiple heads. In other words, is fragmented.
    /// - Expected to be sparse referred. For example, the "visible heads"
    ///   will refer to a bounded subset in this group.
    pub const NON_MASTER: Self = Self(1);

    pub const ALL: [Self; 2] = [Self::MASTER, Self::NON_MASTER];

    // 4 Groups are available. Used 2. Reserved 2.
    const BITS: u32 = 2;
    pub(crate) const MAX: usize = 1 << Self::BITS;

    /// The first [`Id`] in this group.
    pub fn min_id(self) -> Id {
        Id((self.0 as u64) << (64 - Self::BITS))
    }

    /// The maximum [`Id`] in this group.
    pub fn max_id(self) -> Id {
        self.min_id() + ((1u64 << (64 - Self::BITS)) - 1)
    }
}

impl Id {
    /// The [`Group`] of an Id.
    pub fn group(self) -> Group {
        let group = (self.0 >> (64 - Group::BITS)) as usize;
        debug_assert!(group < Group::MAX);
        Group(group)
    }

    /// Similar to `self..=other`.
    pub fn to(self, other: Id) -> IdIter {
        IdIter {
            current: self,
            end: other,
        }
    }

    /// Convert to a byte array. Useful for indexedlog range query.
    pub fn to_bytearray(self) -> [u8; 8] {
        // The field can be used for index range query. So it has to be BE.
        unsafe { std::mem::transmute(self.0.to_be()) }
    }

    /// Similar to `to_bytearray`, but insert a `prefix` at the head.
    /// Useful for segment queries where `level` is the `prefix`.
    pub(crate) fn to_prefixed_bytearray(self, prefix: u8) -> [u8; 9] {
        let a = self.to_bytearray();
        [prefix, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]]
    }

    pub const MAX: Self = Self(u64::max_value());
    pub const MIN: Self = Self(0);
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let group = self.group();
        if group == Group::NON_MASTER {
            write!(f, "N")?;
        }
        write!(f, "{}", self.0 - group.min_id().0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ops::Add<u64> for Id {
    type Output = Id;

    fn add(self, other: u64) -> Self {
        Self(self.0 + other)
    }
}

impl ops::Sub<u64> for Id {
    type Output = Id;

    fn sub(self, other: u64) -> Self {
        Self(self.0 - other)
    }
}

// Consider replacing this with iter::Step once it's stable.
pub struct IdIter {
    current: Id,
    end: Id,
}

impl Iterator for IdIter {
    type Item = Id;

    fn next(&mut self) -> Option<Id> {
        if self.current > self.end {
            None
        } else {
            let result = self.current;
            self.current = self.current + 1;
            Some(result)
        }
    }
}
