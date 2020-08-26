/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::hints::Flags;
use super::{Hints, NameIter, NameSet, NameSetQuery};
use crate::fmt::write_debug;
use crate::Result;
use crate::VertexName;
use std::any::Any;
use std::fmt;

/// Union of 2 sets.
///
/// The order is preserved. The first set is iterated first, then the second set
/// is iterated, with duplicated names skipped.
pub struct UnionSet {
    sets: [NameSet; 2],
    hints: Hints,
}

impl UnionSet {
    pub fn new(lhs: NameSet, rhs: NameSet) -> Self {
        let hints = if lhs.hints().is_id_map_compatible(rhs.hints())
            && lhs.hints().is_dag_compatible(rhs.hints())
        {
            let hints = Hints::new_inherit_idmap_dag(lhs.hints());
            if let (Some(id1), Some(id2)) = (lhs.hints().min_id(), rhs.hints().min_id()) {
                hints.set_min_id(id1.min(id2));
            }
            if let (Some(id1), Some(id2)) = (lhs.hints().max_id(), rhs.hints().max_id()) {
                hints.set_max_id(id1.max(id2));
            }
            hints.add_flags(lhs.hints().flags() & rhs.hints().flags() & Flags::ANCESTORS);
            hints
        } else {
            Hints::default()
        };
        if lhs.hints().contains(Flags::FILTER) || rhs.hints().contains(Flags::FILTER) {
            hints.add_flags(Flags::FILTER);
        }
        Self {
            sets: [lhs, rhs],
            hints,
        }
    }
}

impl NameSetQuery for UnionSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        debug_assert_eq!(self.sets.len(), 2);
        let set0 = self.sets[0].clone();
        let iter =
            self.sets[0]
                .iter()?
                .chain(self.sets[1].iter()?.filter(move |name| match name {
                    Ok(name) => set0.contains(name).ok() != Some(true),
                    _ => true,
                }));
        Ok(Box::new(iter))
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        debug_assert_eq!(self.sets.len(), 2);
        let set0 = self.sets[0].clone();
        let iter = self.sets[1]
            .iter_rev()?
            .filter(move |name| match name {
                Ok(name) => set0.contains(name).ok() != Some(true),
                _ => true,
            })
            .chain(self.sets[0].iter_rev()?);
        Ok(Box::new(iter))
    }

    fn count(&self) -> Result<usize> {
        debug_assert_eq!(self.sets.len(), 2);
        // This is more efficient if sets[0] is a large set that has a fast path
        // for "count()".
        let mut count = self.sets[0].count()?;
        for name in self.sets[1].iter()? {
            if !self.sets[0].contains(&name?)? {
                count += 1;
            }
        }
        Ok(count)
    }

    fn is_empty(&self) -> Result<bool> {
        for set in &self.sets {
            if !set.is_empty()? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        for set in &self.sets {
            if set.contains(name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

impl fmt::Debug for UnionSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<or")?;
        write_debug(f, &self.sets[0])?;
        write_debug(f, &self.sets[1])?;
        write!(f, ">")
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use super::*;
    use std::collections::HashSet;

    fn union(a: &[u8], b: &[u8]) -> UnionSet {
        let a = NameSet::from_query(VecQuery::from_bytes(a));
        let b = NameSet::from_query(VecQuery::from_bytes(b));
        UnionSet::new(a, b)
    }

    #[test]
    fn test_union_basic() -> Result<()> {
        // 'a' overlaps with 'b'. UnionSet should de-duplicate items.
        let set = union(b"\x11\x33\x22", b"\x44\x11\x55\x33");
        check_invariants(&set)?;
        assert_eq!(shorten_iter(set.iter()), ["11", "33", "22", "44", "55"]);
        assert_eq!(shorten_iter(set.iter_rev()), ["55", "44", "22", "33", "11"]);
        assert!(!set.is_empty()?);
        assert_eq!(set.count()?, 5);
        assert_eq!(shorten_name(set.first()?.unwrap()), "11");
        assert_eq!(shorten_name(set.last()?.unwrap()), "55");
        for &b in b"\x11\x22\x33\x44\x55".iter() {
            assert!(set.contains(&to_name(b))?);
        }
        for &b in b"\x66\x77\x88".iter() {
            assert!(!set.contains(&to_name(b))?);
        }
        Ok(())
    }

    quickcheck::quickcheck! {
        fn test_union_quickcheck(a: Vec<u8>, b: Vec<u8>) -> bool {
            let set = union(&a, &b);
            check_invariants(&set).unwrap();

            let count = set.count().unwrap();
            assert!(count <= a.len() + b.len());

            let set2: HashSet<_> = a.iter().chain(b.iter()).cloned().collect();
            assert_eq!(count, set2.len());

            assert!(a.iter().all(|&b| set.contains(&to_name(b)).ok() == Some(true)));
            assert!(b.iter().all(|&b| set.contains(&to_name(b)).ok() == Some(true)));

            true
        }
    }
}
