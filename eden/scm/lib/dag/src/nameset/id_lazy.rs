/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::hints::Flags;
use super::id_static::IdStaticSet;
use super::{Hints, NameIter, NameSetQuery};
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::spanset::SpanSet;
use crate::Group;
use crate::Id;
use crate::Result;
use crate::VertexName;
use indexmap::IndexSet;
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// A set backed by a lazy iterator of Ids.
pub struct IdLazySet {
    // Mutex: iter() does not take &mut self.
    // Arc: iter() result does not have a lifetime on this struct.
    inner: Arc<Mutex<Inner>>,
    pub map: Arc<dyn IdConvert + Send + Sync>,
    pub(crate) dag: Arc<dyn DagAlgorithm + Send + Sync>,
    hints: Hints,
}

struct Inner {
    iter: Box<dyn Iterator<Item = Result<Id>> + Send + Sync>,
    visited: IndexSet<Id>,
    state: State,
}

impl Inner {
    fn load_more(&mut self, n: usize, mut out: Option<&mut Vec<Id>>) -> Result<()> {
        if matches!(self.state, State::Complete | State::Error) {
            return Ok(());
        }
        for _ in 0..n {
            match self.iter.next() {
                Some(Ok(id)) => {
                    if let Some(ref mut out) = out {
                        out.push(id);
                    }
                    self.visited.insert(id);
                }
                None => {
                    self.state = State::Complete;
                    break;
                }
                Some(Err(err)) => {
                    self.state = State::Error;
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum State {
    Incomplete,
    Complete,
    Error,
}

pub struct Iter {
    inner: Arc<Mutex<Inner>>,
    index: usize,
    map: Arc<dyn IdConvert + Send + Sync>,
}

impl Iterator for Iter {
    type Item = Result<VertexName>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            match inner.state {
                State::Error => break None,
                State::Complete if inner.visited.len() <= self.index => break None,
                State::Complete | State::Incomplete => {
                    match inner.visited.get_index(self.index) {
                        Some(&id) => {
                            self.index += 1;
                            match self.map.vertex_name(id) {
                                Err(err) => {
                                    inner.state = State::Error;
                                    return Some(Err(err));
                                }
                                Ok(vertex) => {
                                    break Some(Ok(vertex));
                                }
                            }
                        }
                        None => {
                            // Data not available. Load more.
                            if let Err(err) = inner.load_more(1, None) {
                                return Some(Err(err));
                            }
                        }
                    }
                }
            }
        }
    }
}

struct DebugId {
    id: Id,
    name: Option<VertexName>,
}

impl fmt::Debug for DebugId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(name) = &self.name {
            fmt::Debug::fmt(&name, f)?;
            write!(f, "+{:?}", self.id)?;
        } else {
            write!(f, "{:?}", self.id)?;
        }
        Ok(())
    }
}

impl fmt::Debug for IdLazySet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<lazy ")?;
        let inner = self.inner.lock().unwrap();
        let limit = f.width().unwrap_or(3);
        f.debug_list()
            .entries(inner.visited.iter().take(limit).map(|&id| DebugId {
                id,
                name: self.map.vertex_name(id).ok(),
            }))
            .finish()?;
        let remaining = inner.visited.len().max(limit) - limit;
        match (remaining, inner.state) {
            (0, State::Incomplete) => f.write_str(" + ? more")?,
            (n, State::Incomplete) => write!(f, "+ {} + ? more", n)?,
            (0, _) => (),
            (n, _) => write!(f, " + {} more", n)?,
        }
        f.write_str(">")?;
        Ok(())
    }
}

impl IdLazySet {
    pub fn from_iter_idmap_dag<I>(
        names: I,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Self
    where
        I: IntoIterator<Item = Result<Id>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        let iter = names.into_iter();
        let inner = Inner {
            iter: Box::new(iter),
            visited: IndexSet::new(),
            state: State::Incomplete,
        };
        let hints = Hints::new_with_idmap_dag(map.clone(), dag.clone());
        Self {
            inner: Arc::new(Mutex::new(inner)),
            map,
            dag,
            hints,
        }
    }

    /// Convert to an IdStaticSet.
    pub fn to_static(&self) -> Result<IdStaticSet> {
        let inner = self.load_all()?;
        let mut spans = SpanSet::empty();
        for &id in inner.visited.iter() {
            spans.push(id);
        }
        Ok(IdStaticSet::from_spans_idmap_dag(
            spans,
            self.map.clone(),
            self.dag.clone(),
        ))
    }

    fn load_all(&self) -> Result<MutexGuard<Inner>> {
        let mut inner = self.inner.lock().unwrap();
        inner.load_more(usize::max_value(), None)?;
        Ok(inner)
    }
}

impl NameSetQuery for IdLazySet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        let inner = self.inner.clone();
        let map = self.map.clone();
        let iter = Iter {
            inner,
            index: 0,
            map,
        };
        Ok(Box::new(iter))
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        let inner = self.load_all()?;
        let map = self.map.clone();
        let iter = inner
            .visited
            .clone()
            .into_iter()
            .rev()
            .map(move |id| map.vertex_name(id));
        Ok(Box::new(iter) as Box<dyn NameIter>)
    }

    fn count(&self) -> Result<usize> {
        let inner = self.load_all()?;
        Ok(inner.visited.len())
    }

    fn last(&self) -> Result<Option<VertexName>> {
        let inner = self.load_all()?;
        match inner.visited.iter().rev().nth(0) {
            Some(&id) => Ok(Some(self.map.vertex_name(id)?)),
            None => Ok(None),
        }
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        let id = match self.map.vertex_id_with_max_group(name, Group::NON_MASTER)? {
            None => {
                return Ok(false);
            }
            Some(id) => id,
        };
        if inner.visited.contains(&id) {
            return Ok(true);
        } else {
            let mut loaded = Vec::new();
            loop {
                // Fast paths.
                if let Some(&last_id) = inner.visited.iter().rev().next() {
                    let hints = self.hints();
                    if hints.contains(Flags::ID_DESC) {
                        if last_id < id {
                            return Ok(false);
                        }
                    } else if hints.contains(Flags::ID_ASC) {
                        if last_id > id {
                            return Ok(false);
                        }
                    }
                }
                loaded.clear();
                inner.load_more(1, Some(&mut loaded))?;
                debug_assert!(loaded.len() <= 1);
                if loaded.is_empty() {
                    break;
                }
                if loaded.first() == Some(&id) {
                    return Ok(true);
                }
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

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        Some(self.map.as_ref() as &dyn IdConvert)
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
pub(crate) mod tests {
    use super::super::tests::*;
    use super::super::NameSet;
    use super::*;
    use crate::tests::dummy_dag::DummyDag;
    use std::collections::HashSet;
    use std::convert::TryInto;

    pub fn lazy_set(a: &[u64]) -> IdLazySet {
        let ids: Vec<Id> = a.iter().map(|i| Id(*i as _)).collect();
        IdLazySet::from_iter_idmap_dag(
            ids.into_iter().map(Ok),
            Arc::new(StrIdMap),
            Arc::new(DummyDag),
        )
    }

    pub fn lazy_set_inherit(a: &[u64], set: &IdLazySet) -> IdLazySet {
        let ids: Vec<Id> = a.iter().map(|i| Id(*i as _)).collect();
        IdLazySet::from_iter_idmap_dag(ids.into_iter().map(Ok), set.map.clone(), set.dag.clone())
    }

    struct StrIdMap;

    impl IdConvert for StrIdMap {
        fn vertex_id(&self, name: VertexName) -> Result<Id> {
            let slice: [u8; 8] = name.as_ref().try_into().unwrap();
            let id = u64::from_le(unsafe { std::mem::transmute(slice) });
            Ok(Id(id))
        }
        fn vertex_id_with_max_group(
            &self,
            name: &VertexName,
            _max_group: Group,
        ) -> Result<Option<Id>> {
            if name.as_ref().len() == 8 {
                let id = self.vertex_id(name.clone())?;
                Ok(Some(id))
            } else {
                Ok(None)
            }
        }
        fn vertex_name(&self, id: Id) -> Result<VertexName> {
            let buf: [u8; 8] = unsafe { std::mem::transmute(id.0.to_le()) };
            Ok(VertexName::copy_from(&buf))
        }
        fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
            Ok(name.as_ref().len() == 8)
        }
    }

    #[test]
    fn test_id_lazy_basic() -> Result<()> {
        let set = lazy_set(&[0x11, 0x33, 0x22, 0x77, 0x55]);
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
    fn test_hints_fast_paths() -> Result<()> {
        let set = lazy_set(&[0x20, 0x50, 0x30, 0x70]);

        // Incorrect hints, but useful for testing.
        set.hints().add_flags(Flags::ID_ASC);

        let v = |i: u64| -> VertexName { StrIdMap.vertex_name(Id(i)).unwrap() };
        assert!(set.contains(&v(0x20))?);
        assert!(set.contains(&v(0x50))?);
        assert!(!set.contains(&v(0x30))?);

        set.hints().add_flags(Flags::ID_DESC);
        assert!(set.contains(&v(0x30))?);
        assert!(!set.contains(&v(0x70))?);

        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = lazy_set(&[0]);
        assert_eq!(format!("{:?}", set), "<lazy [] + ? more>");
        set.count().unwrap();
        assert_eq!(format!("{:?}", set), "<lazy [0000000000000000+0]>");

        let set = lazy_set(&[1, 3, 2]);
        assert_eq!(format!("{:?}", &set), "<lazy [] + ? more>");
        let mut iter = set.iter().unwrap();
        iter.next();
        assert_eq!(
            format!("{:?}", &set),
            "<lazy [0100000000000000+1] + ? more>"
        );
        iter.next();
        assert_eq!(
            format!("{:?}", &set),
            "<lazy [0100000000000000+1, 0300000000000000+3] + ? more>"
        );
        iter.next();
        assert_eq!(format!("{:2.2?}", &set), "<lazy [01+1, 03+3]+ 1 + ? more>");
        iter.next();
        assert_eq!(format!("{:1.3?}", &set), "<lazy [010+1] + 2 more>");
    }

    #[test]
    fn test_flatten() {
        let set1 = lazy_set(&[3, 2, 4]);
        let set2 = lazy_set_inherit(&[3, 7, 6], &set1);
        let set1 = NameSet::from_query(set1);
        let set2 = NameSet::from_query(set2);

        // Show flatten by names, and flatten by ids.
        // The first should be <static ...>, the second should be <spans ...>.
        let show = |set: NameSet| {
            [
                format!("{:5.2?}", set.flatten_names().unwrap()),
                format!("{:5.2?}", set.flatten().unwrap()),
            ]
        };

        assert_eq!(
            show(set1.clone() | set2.clone()),
            [
                "<static [03, 02, 04, 07, 06]>",
                "<spans [06:07+6:7, 02:04+2:4]>"
            ]
        );
        assert_eq!(
            show(set1.clone() & set2.clone()),
            ["<static [03]>", "<spans [03+3]>"]
        );
        assert_eq!(
            show(set1.clone() - set2.clone()),
            ["<static [02, 04]>", "<spans [04+4, 02+2]>"]
        );
    }

    quickcheck::quickcheck! {
        fn test_id_lazy_quickcheck(a: Vec<u64>) -> bool {
            let set = lazy_set(&a);
            check_invariants(&set).unwrap();

            let count = set.count().unwrap();
            assert!(count <= a.len());

            let set2: HashSet<_> = a.iter().cloned().collect();
            assert_eq!(count, set2.len());

            true
        }
    }
}
