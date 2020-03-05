/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{NameIter, NameSet, NameSetQuery};
use crate::VertexName;
use anyhow::Result;
use std::any::Any;
use std::fmt;

/// Subset of `lhs` that does not overlap with `rhs`.
///
/// The iteration order is defined by `lhs`.
pub struct DifferenceSet {
    lhs: NameSet,
    rhs: NameSet,
}

struct Iter {
    iter: Box<dyn NameIter>,
    rhs: NameSet,
}

impl NameIter for Iter {}

impl DifferenceSet {
    pub fn new(lhs: NameSet, rhs: NameSet) -> Self {
        Self { lhs, rhs }
    }
}

impl NameSetQuery for DifferenceSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        let iter = Iter {
            iter: self.lhs.iter()?,
            rhs: self.rhs.clone(),
        };
        Ok(Box::new(iter))
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        let iter = Iter {
            iter: self.lhs.iter_rev()?,
            rhs: self.rhs.clone(),
        };
        Ok(Box::new(iter))
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        Ok(self.lhs.contains(name)? && !self.rhs.contains(name)?)
    }

    fn is_topo_sorted(&self) -> bool {
        self.lhs.is_topo_sorted()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl fmt::Debug for DifferenceSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<difference {:?} {:?}>", &self.lhs, &self.rhs)
    }
}

impl Iterator for Iter {
    type Item = Result<VertexName>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let result = NameIter::next(self.iter.as_mut());
            if let Some(Ok(ref name)) = result {
                match self.rhs.contains(&name) {
                    Err(err) => break Some(Err(err)),
                    Ok(true) => continue,
                    _ => (),
                }
            }
            break result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use super::*;

    fn difference(a: &[u8], b: &[u8]) -> DifferenceSet {
        let a = NameSet::from_query(VecQuery::from_bytes(a));
        let b = NameSet::from_query(VecQuery::from_bytes(b));
        DifferenceSet { lhs: a, rhs: b }
    }

    #[test]
    fn test_difference_basic() -> Result<()> {
        let set = difference(b"\x11\x33\x55\x22\x44", b"\x44\x33\x66");
        check_invariants(&set)?;
        assert_eq!(shorten_iter(set.iter()), ["11", "55", "22"]);
        assert_eq!(shorten_iter(set.iter_rev()), ["22", "55", "11"]);
        assert!(!set.is_empty()?);
        assert_eq!(set.count()?, 3);
        assert_eq!(shorten_name(set.first()?.unwrap()), "11");
        assert_eq!(shorten_name(set.last()?.unwrap()), "22");
        for &b in b"\x11\x22\x55".iter() {
            assert!(set.contains(&to_name(b))?);
        }
        for &b in b"\x33\x44\x66".iter() {
            assert!(!set.contains(&to_name(b))?);
        }
        Ok(())
    }

    quickcheck::quickcheck! {
        fn test_difference_quickcheck(a: Vec<u8>, b: Vec<u8>) -> bool {
            let set = difference(&a, &b);
            check_invariants(&set).unwrap();

            let count = set.count().unwrap();
            assert!(count <= a.len());

            assert!(b.iter().all(|&b| set.contains(&to_name(b)).ok() == Some(false)));

            true
        }
    }
}
