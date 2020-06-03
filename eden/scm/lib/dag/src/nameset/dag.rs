/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::hints::Flags;
use super::{Hints, NameIter, NameSetQuery};
use crate::ops::IdConvert;
use crate::spanset::{SpanSet, SpanSetIter};
use crate::Group;
use crate::VertexName;
use anyhow::Result;
use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// A set backed by [`SpanSet`] + [`IdMap`].
/// Efficient for DAG calculation.
pub struct DagSet {
    pub(crate) spans: SpanSet,
    pub(crate) map: Arc<dyn IdConvert + Send + Sync>,
    hints: Hints,
}

struct Iter {
    iter: SpanSetIter<SpanSet>,
    map: Arc<dyn IdConvert + Send + Sync>,
    reversed: bool,
}

impl Iterator for Iter {
    type Item = Result<VertexName>;

    fn next(&mut self) -> Option<Self::Item> {
        let map = &self.map;
        if self.reversed {
            self.iter.next_back()
        } else {
            self.iter.next()
        }
        .map(|id| map.vertex_name(id))
    }
}

impl NameIter for Iter {}

impl fmt::Debug for DagSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<dag [{:?}]>", &self.spans)
    }
}

impl DagSet {
    pub(crate) fn from_spans_idmap(spans: SpanSet, map: Arc<dyn IdConvert + Send + Sync>) -> Self {
        let hints = Hints::default();
        hints.add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
        hints.set_id_map(&map);
        if spans.is_empty() {
            hints.add_flags(Flags::EMPTY | Flags::ID_ASC);
        } else {
            hints.set_min_id(spans.min().unwrap());
            hints.set_max_id(spans.max().unwrap());
        }
        Self { spans, map, hints }
    }
}

impl NameSetQuery for DagSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        let iter: Iter = Iter {
            iter: self.spans.clone().into_iter(),
            map: self.map.clone(),
            reversed: false,
        };
        Ok(Box::new(iter))
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        let iter: Iter = Iter {
            iter: self.spans.clone().into_iter(),
            map: self.map.clone(),
            reversed: true,
        };
        Ok(Box::new(iter))
    }

    fn count(&self) -> Result<usize> {
        Ok(self.spans.count() as usize)
    }

    fn first(&self) -> Result<Option<VertexName>> {
        debug_assert_eq!(self.spans.max(), self.spans.iter().nth(0));
        match self.spans.max() {
            Some(id) => {
                let map = &self.map;
                let name = map.vertex_name(id)?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }

    fn last(&self) -> Result<Option<VertexName>> {
        debug_assert_eq!(self.spans.min(), self.spans.iter().rev().nth(0));
        match self.spans.min() {
            Some(id) => {
                let map = &self.map;
                let name = map.vertex_name(id)?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }

    fn is_empty(&self) -> Result<bool> {
        Ok(self.spans.is_empty())
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        let result = match self.map.vertex_id_with_max_group(name, Group::NON_MASTER)? {
            Some(id) => self.spans.contains(id),
            None => false,
        };
        Ok(result)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::tests::*;
    use super::*;
    use crate::tests::build_segments;
    use crate::DagAlgorithm;
    use crate::NameDag;
    use std::ops::Deref;

    /// Test with a predefined DAG.
    pub(crate) fn with_dag<R, F: Fn(&NameDag) -> R>(func: F) -> R {
        let built = build_segments(
            r#"
            A--B--C--D
                \--E--F--G"#,
            "D G",
            2,
        );
        //  0--1--2--3
        //      \--4--5--6
        func(&built.name_dag)
    }

    #[test]
    fn test_dag_invariants() -> Result<()> {
        with_dag(|dag| {
            let bef = dag.range("B".into(), "F".into())?;
            check_invariants(bef.deref())?;

            Ok(())
        })
    }

    #[test]
    fn test_dag_fast_paths() -> Result<()> {
        with_dag(|dag| {
            let abcd = dag.ancestors("D".into())?;
            let abefg = dag.ancestors("G".into())?;

            let ab = abcd.intersection(&abefg);
            check_invariants(ab.deref())?;

            assert!(abcd.contains(&vec![b'A'].into())?);
            assert!(!abcd.contains(&vec![b'E'].into())?);

            // should not be "<and <...> <...>>"
            assert_eq!(format!("{:?}", &ab), "<dag [0 1]>");

            let abcdefg = abcd.union(&abefg);
            check_invariants(abcd.deref())?;
            // should not be "<or <...> <...>>"
            assert_eq!(format!("{:?}", &abcdefg), "<dag [0..=6]>");

            let cd = abcd.difference(&abefg);
            check_invariants(cd.deref())?;
            // should not be "<difference <...> <...>>"
            assert_eq!(format!("{:?}", &cd), "<dag [2 3]>");

            Ok(())
        })
    }

    #[test]
    fn test_dag_no_fast_paths() -> Result<()> {
        with_dag(|dag1| -> Result<()> {
            with_dag(|dag2| -> Result<()> {
                let abcd = dag1.ancestors("D".into())?;
                let abefg = dag2.ancestors("G".into())?;

                // Since abcd and abefg are from 2 "separate" Dags, fast paths should not
                // be used for intersection, union, and difference.

                let ab = abcd.intersection(&abefg);
                check_invariants(ab.deref())?;
                // should not be "<dag ...>"
                assert_eq!(
                    format!("{:?}", &ab),
                    "<and <dag [0..=3]> <dag [0 1 4 5 6]>>"
                );

                let abcdefg = abcd.union(&abefg);
                check_invariants(abcd.deref())?;
                // should not be "<dag ...>"
                assert_eq!(
                    format!("{:?}", &abcdefg),
                    "<or <dag [0..=3]> <dag [0 1 4 5 6]>>"
                );

                let cd = abcd.difference(&abefg);
                check_invariants(cd.deref())?;
                // should not be "<dag ...>"
                assert_eq!(
                    format!("{:?}", &cd),
                    "<difference <dag [0..=3]> <dag [0 1 4 5 6]>>"
                );

                Ok(())
            })
        })
    }

    #[test]
    fn test_dag_all() -> Result<()> {
        with_dag(|dag| {
            let all = dag.all()?;
            assert_eq!(format!("{:?}", &all), "<dag [0..=6]>");

            let ac = "A C".into();
            let intersection = all.intersection(&ac);
            // should not be "<and ...>"
            assert_eq!(format!("{:?}", &intersection), "<[A C]>");
            Ok(())
        })
    }

    #[test]
    fn test_sort() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let set = "G C A E".into();
            let sorted = dag.sort(&set)?;
            assert_eq!(format!("{:?}", &sorted), "<dag [0 2 4 6]>");
            Ok(())
        })
    }
}
