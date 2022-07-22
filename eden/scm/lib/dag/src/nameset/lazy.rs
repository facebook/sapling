/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use futures::lock::Mutex;
use futures::lock::MutexGuard;
use futures::StreamExt;
use indexmap::IndexSet;

use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use crate::Result;
use crate::VertexName;

/// A set backed by a lazy iterator of names.
pub struct LazySet {
    inner: Arc<Mutex<Inner>>,
    hints: Hints,
}

struct Inner {
    iter: BoxVertexStream,
    visited: IndexSet<VertexName>,
    state: State,
}

impl Inner {
    async fn load_more(&mut self, n: usize, mut out: Option<&mut Vec<VertexName>>) -> Result<()> {
        if matches!(self.state, State::Complete | State::Error) {
            return Ok(());
        }
        for _ in 0..n {
            match self.iter.next().await {
                Some(Ok(name)) => {
                    if let Some(ref mut out) = out {
                        out.push(name.clone());
                    }
                    self.visited.insert(name);
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
}

impl Iter {
    async fn next(&mut self) -> Option<Result<VertexName>> {
        loop {
            let mut inner = self.inner.lock().await;
            match inner.state {
                State::Error => break None,
                State::Complete if inner.visited.len() <= self.index => break None,
                State::Complete | State::Incomplete => {
                    let value = inner.visited.get_index(self.index).cloned();
                    match value {
                        Some(value) => {
                            self.index += 1;
                            break Some(Ok(value));
                        }
                        None => {
                            // Data not available. Load more.
                            if let Err(err) = inner.load_more(1, None).await {
                                return Some(Err(err));
                            }
                            continue;
                        }
                    }
                }
            }
        }
    }

    fn into_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |mut state| async move {
            let result = state.next().await;
            result.map(|r| (r, state))
        }))
    }
}

impl fmt::Debug for LazySet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<lazy ")?;
        if let Some(inner) = self.inner.try_lock() {
            let limit = f.width().unwrap_or(3);
            f.debug_list()
                .entries(inner.visited.iter().take(limit))
                .finish()?;
            let remaining = inner.visited.len().max(limit) - limit;
            match (remaining, inner.state) {
                (0, State::Incomplete) => f.write_str(" + ? more")?,
                (n, State::Incomplete) => write!(f, "+ {} + ? more", n)?,
                (0, _) => {}
                (n, _) => write!(f, " + {} more", n)?,
            }
        } else {
            f.write_str(" ?")?;
        }
        f.write_str(">")?;
        Ok(())
    }
}

impl LazySet {
    pub fn from_iter<I>(names: I, hints: Hints) -> Self
    where
        I: IntoIterator<Item = Result<VertexName>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        let stream = futures::stream::iter(names);
        Self::from_stream(Box::pin(stream), hints)
    }

    pub fn from_stream(names: BoxVertexStream, hints: Hints) -> Self {
        let inner = Inner {
            iter: names,
            visited: IndexSet::new(),
            state: State::Incomplete,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
            hints,
        }
    }

    async fn load_all(&self) -> Result<MutexGuard<'_, Inner>> {
        let mut inner = self.inner.lock().await;
        inner.load_more(usize::max_value(), None).await?;
        Ok(inner)
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for LazySet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let inner = self.inner.clone();
        let iter = Iter { inner, index: 0 };
        Ok(iter.into_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let inner = self.load_all().await?;
        let iter = inner.visited.clone().into_iter().rev().map(Ok);
        let stream = futures::stream::iter(iter);
        Ok(Box::pin(stream))
    }

    async fn count(&self) -> Result<usize> {
        let inner = self.load_all().await?;
        Ok(inner.visited.len())
    }

    async fn last(&self) -> Result<Option<VertexName>> {
        let inner = self.load_all().await?;
        Ok(inner.visited.iter().rev().nth(0).cloned())
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        let mut inner = self.inner.lock().await;
        if inner.visited.contains(name) {
            return Ok(true);
        } else {
            let mut loaded = Vec::new();
            loop {
                loaded.clear();
                inner.load_more(1, Some(&mut loaded)).await?;
                debug_assert!(loaded.len() <= 1);
                if loaded.is_empty() {
                    break;
                }
                if loaded.first() == Some(name) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        let inner = self.inner.lock().await;
        if inner.visited.contains(name) {
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
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::tests::*;
    use super::*;

    fn lazy_set(a: &[u8]) -> LazySet {
        LazySet::from_iter(
            a.to_vec().into_iter().map(|b| Ok(to_name(b))),
            Hints::default(),
        )
    }

    #[test]
    fn test_lazy_basic() -> Result<()> {
        let set = lazy_set(b"\x11\x33\x22\x77\x22\x55\x11");
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
    fn test_debug() {
        let set = lazy_set(b"");
        assert_eq!(format!("{:?}", &set), "<lazy [] + ? more>");
        nb(set.count()).unwrap();
        assert_eq!(format!("{:?}", &set), "<lazy []>");

        let set = lazy_set(b"\x11\x33\x22");
        assert_eq!(format!("{:?}", &set), "<lazy [] + ? more>");
        let mut iter = ni(set.iter()).unwrap();
        iter.next();
        assert_eq!(format!("{:?}", &set), "<lazy [1111] + ? more>");
        iter.next();
        assert_eq!(format!("{:?}", &set), "<lazy [1111, 3333] + ? more>");
        iter.next();
        assert_eq!(format!("{:2.2?}", &set), "<lazy [11, 33]+ 1 + ? more>");
        iter.next();
        assert_eq!(format!("{:1.3?}", &set), "<lazy [111] + 2 more>");
    }

    quickcheck::quickcheck! {
        fn test_lazy_quickcheck(a: Vec<u8>) -> bool {
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
