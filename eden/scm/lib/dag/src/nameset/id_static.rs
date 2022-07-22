/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use nonblocking::non_blocking_result;

use super::hints::Flags;
use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::protocol::disable_remote_protocol;
use crate::Group;
use crate::IdSet;
use crate::IdSetIter;
use crate::IdSpan;
use crate::Result;
use crate::VertexName;

/// A set backed by [`IdSet`] + [`IdMap`].
/// Efficient for DAG calculation.
pub struct IdStaticSet {
    pub(crate) spans: IdSet,
    pub(crate) map: Arc<dyn IdConvert + Send + Sync>,
    pub(crate) dag: Arc<dyn DagAlgorithm + Send + Sync>,
    hints: Hints,
}

struct Iter {
    iter: IdSetIter<IdSet>,
    map: Arc<dyn IdConvert + Send + Sync>,
    reversed: bool,
    buf: Vec<Result<VertexName>>,
}

impl Iter {
    fn into_box_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |this| this.next()))
    }

    async fn next(mut self) -> Option<(Result<VertexName>, Self)> {
        if let Some(name) = self.buf.pop() {
            return Some((name, self));
        }
        let map = &self.map;
        let opt_id = if self.reversed {
            self.iter.next_back()
        } else {
            self.iter.next()
        };
        match opt_id {
            None => None,
            Some(id) => {
                let contains = map
                    .contains_vertex_id_locally(&[id])
                    .await
                    .unwrap_or_default();
                if contains == &[true] {
                    Some((map.vertex_name(id).await, self))
                } else {
                    // On demand prefetch in batch.
                    let batch_size = 131072;
                    let mut ids = Vec::with_capacity(batch_size);
                    ids.push(id);
                    for _ in ids.len()..batch_size {
                        if let Some(id) = if self.reversed {
                            self.iter.next_back()
                        } else {
                            self.iter.next()
                        } {
                            ids.push(id);
                        } else {
                            break;
                        }
                    }
                    ids.reverse();
                    self.buf = match self.map.vertex_name_batch(&ids).await {
                        Err(e) => return Some((Err(e), self)),
                        Ok(names) => names,
                    };
                    if self.buf.len() != ids.len() {
                        let result =
                            crate::errors::bug("vertex_name_batch does not return enough items");
                        return Some((result, self));
                    }
                    let name = self.buf.pop().expect("buf is not empty");
                    Some((name, self))
                }
            }
        }
    }
}

struct DebugSpan {
    span: IdSpan,
    low_name: Option<VertexName>,
    high_name: Option<VertexName>,
}

impl fmt::Debug for DebugSpan {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (
            self.span.low == self.span.high,
            &self.low_name,
            &self.high_name,
        ) {
            (true, Some(name), _) => {
                fmt::Debug::fmt(&name, f)?;
                write!(f, "+{:?}", self.span.low)?;
            }
            (true, None, _) => {
                write!(f, "{:?}", self.span.low)?;
            }
            (false, Some(low), Some(high)) => {
                fmt::Debug::fmt(&low, f)?;
                write!(f, ":")?;
                fmt::Debug::fmt(&high, f)?;
                write!(f, "+{:?}:{:?}", self.span.low, self.span.high)?;
            }
            (false, _, _) => {
                write!(f, "{:?}:{:?}", self.span.low, self.span.high)?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for IdStaticSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<spans ")?;
        let spans = self.spans.as_spans();
        let limit = f.width().unwrap_or(3);
        f.debug_list()
            .entries(spans.iter().take(limit).map(|span| DebugSpan {
                span: *span,
                low_name: disable_remote_protocol(|| {
                    non_blocking_result(self.map.vertex_name(span.low)).ok()
                }),
                high_name: disable_remote_protocol(|| {
                    non_blocking_result(self.map.vertex_name(span.high)).ok()
                }),
            }))
            .finish()?;
        match spans.len().max(limit) - limit {
            0 => {}
            1 => write!(f, " + 1 span")?,
            n => write!(f, " + {} spans", n)?,
        }
        write!(f, ">")?;
        Ok(())
    }
}

impl IdStaticSet {
    pub(crate) fn from_spans_idmap_dag(
        spans: IdSet,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Self {
        let hints = Hints::new_with_idmap_dag(map.clone(), dag.clone());
        hints.add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
        if spans.is_empty() {
            hints.add_flags(Flags::EMPTY);
        } else {
            hints.set_min_id(spans.min().unwrap());
            hints.set_max_id(spans.max().unwrap());
        }
        Self {
            spans,
            map,
            hints,
            dag,
        }
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for IdStaticSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let iter = Iter {
            iter: self.spans.clone().into_iter(),
            map: self.map.clone(),
            reversed: false,
            buf: Default::default(),
        };
        Ok(iter.into_box_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let iter = Iter {
            iter: self.spans.clone().into_iter(),
            map: self.map.clone(),
            reversed: true,
            buf: Default::default(),
        };
        Ok(iter.into_box_stream())
    }

    async fn count(&self) -> Result<usize> {
        Ok(self.spans.count() as usize)
    }

    async fn first(&self) -> Result<Option<VertexName>> {
        debug_assert_eq!(self.spans.max(), self.spans.iter_desc().nth(0));
        match self.spans.max() {
            Some(id) => {
                let map = &self.map;
                let name = map.vertex_name(id).await?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }

    async fn last(&self) -> Result<Option<VertexName>> {
        debug_assert_eq!(self.spans.min(), self.spans.iter_desc().rev().nth(0));
        match self.spans.min() {
            Some(id) => {
                let map = &self.map;
                let name = map.vertex_name(id).await?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }

    async fn is_empty(&self) -> Result<bool> {
        Ok(self.spans.is_empty())
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        let result = match self
            .map
            .vertex_id_with_max_group(name, Group::NON_MASTER)
            .await?
        {
            Some(id) => self.spans.contains(id),
            None => false,
        };
        Ok(result)
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        self.contains(name).await.map(Some)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        Some(self.map.as_ref() as &dyn IdConvert)
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
pub(crate) mod tests {
    use std::ops::Deref;

    use nonblocking::non_blocking_result as r;

    use super::super::tests::*;
    use super::super::NameSet;
    use super::*;
    use crate::tests::build_segments;
    use crate::DagAlgorithm;
    use crate::NameDag;

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
            let bef = r(dag.range("B".into(), "F".into()))?;
            check_invariants(bef.deref())?;

            Ok(())
        })
    }

    #[test]
    fn test_dag_fast_paths() -> Result<()> {
        with_dag(|dag| {
            let abcd = r(dag.ancestors("D".into()))?;
            let abefg = r(dag.ancestors("G".into()))?;

            let ab = abcd.intersection(&abefg);
            check_invariants(ab.deref())?;

            assert!(nb(abcd.contains(&vec![b'A'].into()))?);
            assert!(!nb(abcd.contains(&vec![b'E'].into()))?);

            // should not be "<and <...> <...>>"
            assert_eq!(format!("{:?}", &ab), "<spans [A:B+0:1]>");

            let abcdefg = abcd.union(&abefg);
            check_invariants(abcd.deref())?;
            // should not be "<or <...> <...>>"
            assert_eq!(format!("{:?}", &abcdefg), "<spans [A:G+0:6]>");

            let cd = abcd.difference(&abefg);
            check_invariants(cd.deref())?;
            // should not be "<difference <...> <...>>"
            assert_eq!(format!("{:?}", &cd), "<spans [C:D+2:3]>");

            Ok(())
        })
    }

    #[test]
    fn test_dag_no_fast_paths() -> Result<()> {
        let f = |s: NameSet| -> String { format!("{:?}", s) };
        with_dag(|dag1| -> Result<()> {
            with_dag(|dag2| -> Result<()> {
                let abcd = r(dag1.ancestors("D".into()))?;
                let abefg = r(dag2.ancestors("G".into()))?;

                // Since abcd and abefg are from 2 "separate" Dags, fast paths should not
                // be used for intersection, union, and difference.

                let ab = abcd.intersection(&abefg);
                check_invariants(ab.deref())?;
                // should not be "<spans ...>"
                assert_eq!(
                    format!("{:?}", &ab),
                    "<and <spans [A:D+0:3]> <spans [E:G+4:6, A:B+0:1]>>"
                );

                let abcdefg = abcd.union(&abefg);
                check_invariants(abcd.deref())?;
                // should not be "<spans ...>"
                assert_eq!(
                    format!("{:?}", &abcdefg),
                    "<or <spans [A:D+0:3]> <spans [E:G+4:6, A:B+0:1]>>"
                );

                let cd = abcd.difference(&abefg);
                check_invariants(cd.deref())?;
                // should not be "<spans ...>"
                assert_eq!(
                    format!("{:?}", &cd),
                    "<diff <spans [A:D+0:3]> <spans [E:G+4:6, A:B+0:1]>>"
                );

                // Should not use FULL hint fast paths for "&, |, -" operations, because
                // dag1 and dag2 are not considered compatible.
                let a1 = || r(dag1.all()).unwrap();
                let a2 = || r(dag2.all()).unwrap();
                assert_eq!(f(a1() & a2()), "<and <spans [A:G+0:6]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() | a2()), "<or <spans [A:G+0:6]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() - a2()), "<diff <spans [A:G+0:6]> <spans [A:G+0:6]>>");

                // No fast path for manually constructed StaticSet either, because
                // the StaticSets do not have DAG associated to test compatibility.
                // However, "all & z" is changed to "z & all" for performance.
                let z = || NameSet::from("Z");
                assert_eq!(f(z() & a2()), "<and <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(z() | a2()), "<or <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(z() - a2()), "<diff <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() & z()), "<and <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() | z()), "<or <spans [A:G+0:6]> <static [Z]>>");
                assert_eq!(f(a1() - z()), "<diff <spans [A:G+0:6]> <static [Z]>>");

                // EMPTY fast paths can still be used.
                let e = || NameSet::empty();
                assert_eq!(f(e() & a1()), "<empty>");
                assert_eq!(f(e() | a1()), "<spans [A:G+0:6]>");
                assert_eq!(f(e() - a1()), "<empty>");
                assert_eq!(f(a1() & e()), "<empty>");
                assert_eq!(f(a1() | e()), "<spans [A:G+0:6]>");
                assert_eq!(f(a1() - e()), "<spans [A:G+0:6]>");

                Ok(())
            })
        })
    }

    #[test]
    fn test_dag_all() -> Result<()> {
        with_dag(|dag| {
            let all = r(dag.all())?;
            assert_eq!(format!("{:?}", &all), "<spans [A:G+0:6]>");

            let ac: NameSet = "A C".into();
            let ac = r(dag.sort(&ac))?;

            let intersection = all.intersection(&ac);
            // should not be "<and ...>"
            assert_eq!(format!("{:?}", &intersection), "<spans [C+2, A+0]>");
            Ok(())
        })
    }

    #[test]
    fn test_sort() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let set = "G C A E".into();
            let sorted = r(dag.sort(&set))?;
            assert_eq!(format!("{:?}", &sorted), "<spans [G+6, E+4, C+2] + 1 span>");
            Ok(())
        })
    }

    #[test]
    fn test_dag_hints_ancestors() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let abc = r(dag.ancestors("B C".into()))?;
            let abe = r(dag.common_ancestors("E".into()))?;
            let f: NameSet = "F".into();
            let all = r(dag.all())?;

            assert!(has_ancestors_flag(abc.clone()));
            assert!(has_ancestors_flag(abe.clone()));
            assert!(has_ancestors_flag(all.clone()));
            assert!(has_ancestors_flag(r(dag.roots(abc.clone()))?));
            assert!(has_ancestors_flag(r(dag.parents(all.clone()))?));

            assert!(!has_ancestors_flag(f.clone()));
            assert!(!has_ancestors_flag(r(dag.roots(f.clone()))?));
            assert!(!has_ancestors_flag(r(dag.parents(f.clone()))?));

            Ok(())
        })
    }

    #[test]
    fn test_dag_hints_ancestors_inheritance() -> Result<()> {
        with_dag(|dag1| -> Result<()> {
            with_dag(|dag2| -> Result<()> {
                let abc = r(dag1.ancestors("B C".into()))?;

                // The ANCESTORS flag is kept by 'sort', 'parents', 'roots' on
                // the same dag.
                assert!(has_ancestors_flag(r(dag1.sort(&abc))?));
                assert!(has_ancestors_flag(r(dag1.parents(abc.clone()))?));
                assert!(has_ancestors_flag(r(dag1.roots(abc.clone()))?));

                // The ANCESTORS flag is removed on a different dag, since the
                // different dag does not assume same graph / ancestry
                // relationship.
                assert!(!has_ancestors_flag(r(dag2.sort(&abc))?));
                assert!(!has_ancestors_flag(r(dag2.parents(abc.clone()))?));
                assert!(!has_ancestors_flag(r(dag2.roots(abc.clone()))?));

                Ok(())
            })
        })
    }

    #[test]
    fn test_dag_hints_ancestors_fast_paths() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let bfg: NameSet = "B F G".into();

            // Set the ANCESTORS flag. It's incorrect but make it easier to test fast paths.
            bfg.hints().add_flags(Flags::ANCESTORS);

            // Fast paths are not used if the set is not "bound" to the dag.
            assert_eq!(
                format!("{:?}", r(dag.ancestors(bfg.clone()))?),
                "<static [B, F, G]>"
            );
            assert_eq!(format!("{:?}", r(dag.heads(bfg.clone()))?), "<spans [G+6]>");

            // Binding to the Dag enables fast paths.
            let bfg = r(dag.sort(&bfg))?;
            bfg.hints().add_flags(Flags::ANCESTORS);
            assert_eq!(
                format!("{:?}", r(dag.ancestors(bfg.clone()))?),
                "<spans [F:G+5:6, B+1]>"
            );

            // 'heads' has a fast path that uses 'heads_ancestors' to do the calculation.
            // (in this case the result is incorrect because the hints are wrong).
            assert_eq!(format!("{:?}", r(dag.heads(bfg.clone()))?), "<spans [G+6]>");

            // 'ancestors' has a fast path that returns set as-is.
            // (in this case the result is incorrect because the hints are wrong).
            assert_eq!(
                format!("{:?}", r(dag.ancestors(bfg.clone()))?),
                "<spans [F:G+5:6, B+1]>"
            );

            Ok(())
        })
    }

    fn has_ancestors_flag(set: NameSet) -> bool {
        set.hints().contains(Flags::ANCESTORS)
    }
}
