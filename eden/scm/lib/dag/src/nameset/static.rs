/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{NameIter, NameSetQuery};
use crate::VertexName;
use anyhow::Result;
use indexmap::IndexSet;
use std::any::Any;
use std::fmt;

/// A set backed by a concrete ordered set.
pub struct StaticSet(pub(crate) IndexSet<VertexName>);

type Iter =
    std::iter::Map<indexmap::set::IntoIter<VertexName>, fn(VertexName) -> Result<VertexName>>;

pub(crate) type IterRev = std::iter::Map<
    std::iter::Rev<indexmap::set::IntoIter<VertexName>>,
    fn(VertexName) -> Result<VertexName>,
>;

impl NameIter for Iter {}
impl NameIter for IterRev {}

impl StaticSet {
    pub fn from_names(names: impl IntoIterator<Item = VertexName>) -> Self {
        let names = names.into_iter().collect();
        Self(names)
    }
}

impl NameSetQuery for StaticSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        let iter: Iter = self.0.clone().into_iter().map(Ok);
        Ok(Box::new(iter))
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        let iter: IterRev = self.0.clone().into_iter().rev().map(Ok);
        Ok(Box::new(iter))
    }

    fn count(&self) -> Result<usize> {
        Ok(self.0.len())
    }

    fn is_empty(&self) -> Result<bool> {
        Ok(self.0.is_empty())
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        Ok(self.0.contains(name))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl fmt::Debug for StaticSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<[")?;
        let mut count = 0;
        // Makes the "debug" string bounded so it won't be super long for large StaticSet.
        for name in self.0.iter().take(3) {
            if count > 0 {
                write!(f, " ")?;
            }
            write!(f, "{:?}", &name)?;
            count += 1;
        }
        let remaining = self.0.len() - count;
        if remaining > 0 {
            write!(f, "] + {} more>", remaining)?;
        } else {
            write!(f, "]>")?;
        }
        Ok(())
    }
}

// Test infra is unhappy about 'r#' yet (D20008157).
#[cfg(not(fbcode_build))]
#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use super::*;
    use std::collections::HashSet;

    fn static_set(a: &[u8]) -> StaticSet {
        StaticSet::from_names(a.iter().map(|&b| to_name(b)))
    }

    #[test]
    fn test_static_basic() -> Result<()> {
        let set = static_set(b"\x11\x33\x22\x77\x22\x55\x11");
        check_invariants(&set)?;
        assert_eq!(shorten_iter(set.iter()), ["11", "33", "22", "77", "55"]);
        assert_eq!(shorten_iter(set.iter_rev()), ["55", "77", "22", "33", "11"]);
        assert!(!set.is_empty()?);
        assert_eq!(set.count()?, 5);
        assert_eq!(shorten_name(set.first()?.unwrap()), "11");
        assert_eq!(shorten_name(set.last()?.unwrap()), "55");
        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = static_set(b"");
        assert_eq!(format!("{:?}", set), "<[]>");

        let set = static_set(b"\x11\x33\x22");
        assert_eq!(format!("{:?}", set), "<[1111 3333 2222]>");

        let set = static_set(b"\xaa\x00\xaa\xdd\xee\xdd\x11\x22");
        assert_eq!(format!("{:?}", set), "<[aaaa 0000 dddd] + 3 more>");
    }

    quickcheck::quickcheck! {
        fn test_static_quickcheck(a: Vec<u8>) -> bool {
            let set = static_set(&a);
            check_invariants(&set).unwrap();

            let count = set.count().unwrap();
            assert!(count <= a.len());

            let set2: HashSet<_> = a.iter().cloned().collect();
            assert_eq!(count, set2.len());

            true
        }
    }
}
