/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use indexmap::IndexSet;
use nonblocking::non_blocking_result;

use super::hints::Flags;
use super::id_static::IdStaticSet;
use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::protocol::disable_remote_protocol;
use crate::Group;
use crate::Id;
use crate::IdSet;
use crate::Result;
use crate::VertexName;

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

impl Iter {
    fn into_box_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |this| this.next()))
    }

    async fn next(mut self) -> Option<(Result<VertexName>, Self)> {
        loop {
            let state = {
                let inner = self.inner.lock().unwrap();
                inner.state
            };
            match state {
                State::Error => break None,
                State::Complete if self.inner.lock().unwrap().visited.len() <= self.index => {
                    break None;
                }
                State::Complete | State::Incomplete => {
                    let opt_id = {
                        let inner = self.inner.lock().unwrap();
                        inner.visited.get_index(self.index).cloned()
                    };
                    match opt_id {
                        Some(id) => {
                            self.index += 1;
                            match self.map.vertex_name(id).await {
                                Err(err) => {
                                    self.inner.lock().unwrap().state = State::Error;
                                    return Some((Err(err), self));
                                }
                                Ok(vertex) => {
                                    break Some((Ok(vertex), self));
                                }
                            }
                        }
                        None => {
                            // Data not available. Load more.
                            let more = {
                                let mut inner = self.inner.lock().unwrap();
                                inner.load_more(1, None)
                            };
                            if let Err(err) = more {
                                return Some((Err(err), self));
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
                name: disable_remote_protocol(|| {
                    non_blocking_result(self.map.vertex_name(id)).ok()
                }),
            }))
            .finish()?;
        let remaining = inner.visited.len().max(limit) - limit;
        match (remaining, inner.state) {
            (0, State::Incomplete) => f.write_str(" + ? more")?,
            (n, State::Incomplete) => write!(f, "+ {} + ? more", n)?,
            (0, _) => {}
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
        let mut spans = IdSet::empty();
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

#[async_trait::async_trait]
impl AsyncNameSetQuery for IdLazySet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let inner = self.inner.clone();
        let map = self.map.clone();
        let iter = Iter {
            inner,
            index: 0,
            map,
        };
        Ok(iter.into_box_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let inner = self.load_all()?;
        struct State {
            map: Arc<dyn IdConvert + Send + Sync>,
            iter: Box<dyn Iterator<Item = Id> + Send>,
        }
        let state = State {
            map: self.map.clone(),
            iter: Box::new(inner.visited.clone().into_iter().rev()),
        };
        async fn next(mut state: State) -> Option<(Result<VertexName>, State)> {
            match state.iter.next() {
                None => None,
                Some(id) => {
                    let result = state.map.vertex_name(id).await;
                    Some((result, state))
                }
            }
        }

        let stream = futures::stream::unfold(state, next);
        Ok(Box::pin(stream))
    }

    async fn count(&self) -> Result<usize> {
        let inner = self.load_all()?;
        Ok(inner.visited.len())
    }

    async fn last(&self) -> Result<Option<VertexName>> {
        let opt_id = {
            let inner = self.load_all()?;
            inner.visited.iter().rev().nth(0).cloned()
        };
        match opt_id {
            Some(id) => Ok(Some(self.map.vertex_name(id).await?)),
            None => Ok(None),
        }
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        let id = match self
            .map
            .vertex_id_with_max_group(name, Group::NON_MASTER)
            .await?
        {
            None => {
                return Ok(false);
            }
            Some(id) => id,
        };
        let mut inner = self.inner.lock().unwrap();
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

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        let id = match self
            .map
            .vertex_id_with_max_group(name, Group::NON_MASTER)
            .await?
        {
            None => {
                return Ok(Some(false));
            }
            Some(id) => id,
        };
        let inner = self.inner.lock().unwrap();
        if inner.visited.contains(&id) {
            return Ok(Some(true));
        } else if inner.state != State::Incomplete {
            return Ok(Some(false));
        }
        Ok(None)
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

#[cfg(all(test, feature = "indexedlog-backend"))]
#[allow(clippy::redundant_clone)]
pub(crate) mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering::AcqRel;

    use nonblocking::non_blocking_result as r;

    use super::super::tests::*;
    use super::super::NameSet;
    use super::*;
    use crate::ops::PrefixLookup;
    use crate::tests::dummy_dag::DummyDag;
    use crate::VerLink;

    pub fn lazy_set(a: &[u64]) -> IdLazySet {
        let ids: Vec<Id> = a.iter().map(|i| Id(*i as _)).collect();
        IdLazySet::from_iter_idmap_dag(
            ids.into_iter().map(Ok),
            Arc::new(StrIdMap::new()),
            Arc::new(DummyDag::new()),
        )
    }

    pub fn lazy_set_inherit(a: &[u64], set: &IdLazySet) -> IdLazySet {
        let ids: Vec<Id> = a.iter().map(|i| Id(*i as _)).collect();
        IdLazySet::from_iter_idmap_dag(ids.into_iter().map(Ok), set.map.clone(), set.dag.clone())
    }

    static STR_ID_MAP_ID: AtomicU64 = AtomicU64::new(0);

    struct StrIdMap {
        id: String,
        version: VerLink,
    }

    impl StrIdMap {
        fn new() -> Self {
            Self {
                id: format!("str:{}", STR_ID_MAP_ID.fetch_add(1, AcqRel)),
                version: VerLink::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl PrefixLookup for StrIdMap {
        async fn vertexes_by_hex_prefix(&self, _: &[u8], _: usize) -> Result<Vec<VertexName>> {
            // Dummy implementation.
            Ok(Vec::new())
        }
    }
    #[async_trait::async_trait]
    impl IdConvert for StrIdMap {
        async fn vertex_id(&self, name: VertexName) -> Result<Id> {
            let slice: [u8; 8] = name.as_ref().try_into().unwrap();
            let id = u64::from_le(unsafe { std::mem::transmute(slice) });
            Ok(Id(id))
        }
        async fn vertex_id_with_max_group(
            &self,
            name: &VertexName,
            _max_group: Group,
        ) -> Result<Option<Id>> {
            if name.as_ref().len() == 8 {
                let id = self.vertex_id(name.clone()).await?;
                Ok(Some(id))
            } else {
                Ok(None)
            }
        }
        async fn vertex_name(&self, id: Id) -> Result<VertexName> {
            let buf: [u8; 8] = unsafe { std::mem::transmute(id.0.to_le()) };
            Ok(VertexName::copy_from(&buf))
        }
        async fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
            Ok(name.as_ref().len() == 8)
        }
        fn map_id(&self) -> &str {
            &self.id
        }
        fn map_version(&self) -> &VerLink {
            &self.version
        }
        async fn contains_vertex_id_locally(&self, ids: &[Id]) -> Result<Vec<bool>> {
            Ok(ids.iter().map(|_| true).collect())
        }
        async fn contains_vertex_name_locally(&self, names: &[VertexName]) -> Result<Vec<bool>> {
            Ok(names.iter().map(|name| name.as_ref().len() == 8).collect())
        }
    }

    #[test]
    fn test_id_lazy_basic() -> Result<()> {
        let set = lazy_set(&[0x11, 0x33, 0x22, 0x77, 0x55]);
        check_invariants(&set)?;
        assert_eq!(shorten_iter(ni(set.iter())), ["11", "33", "22", "77", "55"]);
        assert_eq!(
            shorten_iter(ni(set.iter_rev())),
            ["55", "77", "22", "33", "11"]
        );
        assert!(!nb(set.is_empty())?);
        assert_eq!(nb(set.count())?, 5);
        assert_eq!(shorten_name(nb(set.first())?.unwrap()), "11");
        assert_eq!(shorten_name(nb(set.last())?.unwrap()), "55");
        Ok(())
    }

    #[test]
    fn test_hints_fast_paths() -> Result<()> {
        let set = lazy_set(&[0x20, 0x50, 0x30, 0x70]);

        // Incorrect hints, but useful for testing.
        set.hints().add_flags(Flags::ID_ASC);

        let v = |i: u64| -> VertexName { r(StrIdMap::new().vertex_name(Id(i))).unwrap() };
        assert!(nb(set.contains(&v(0x20)))?);
        assert!(nb(set.contains(&v(0x50)))?);
        assert!(!nb(set.contains(&v(0x30)))?);

        set.hints().add_flags(Flags::ID_DESC);
        assert!(nb(set.contains(&v(0x30)))?);
        assert!(!nb(set.contains(&v(0x70)))?);

        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = lazy_set(&[0]);
        assert_eq!(format!("{:?}", set), "<lazy [] + ? more>");
        nb(set.count()).unwrap();
        assert_eq!(format!("{:?}", set), "<lazy [0000000000000000+0]>");

        let set = lazy_set(&[1, 3, 2]);
        assert_eq!(format!("{:?}", &set), "<lazy [] + ? more>");
        let mut iter = ni(set.iter()).unwrap();
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
                format!("{:5.2?}", r(set.flatten_names()).unwrap()),
                format!("{:5.2?}", r(set.flatten()).unwrap()),
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

            let count = nb(set.count()).unwrap();
            assert!(count <= a.len());

            let set2: HashSet<_> = a.iter().cloned().collect();
            assert_eq!(count, set2.len());

            true
        }
    }
}
