/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{Hints, NameIter, NameSetQuery};
use crate::Result;
use crate::VertexName;
use indexmap::IndexSet;
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// A set backed by a lazy iterator of names.
pub struct LazySet {
    inner: Arc<Mutex<Inner>>,
    hints: Hints,
}

struct Inner {
    iter: Box<dyn Iterator<Item = Result<VertexName>> + Send + Sync>,
    visited: IndexSet<VertexName>,
    state: State,
}

impl Inner {
    fn load_more(&mut self, n: usize, mut out: Option<&mut Vec<VertexName>>) -> Result<()> {
        if matches!(self.state, State::Complete | State::Error) {
            return Ok(());
        }
        for _ in 0..n {
            match self.iter.next() {
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
                        Some(value) => {
                            self.index += 1;
                            break Some(Ok(value.clone()));
                        }
                        None => {
                            // Data not available. Load more.
                            if let Err(err) = inner.load_more(1, None) {
                                return Some(Err(err));
                            }
                            continue;
                        }
                    }
                }
            }
        }
    }
}

impl fmt::Debug for LazySet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<lazy ")?;
        let inner = self.inner.lock().unwrap();
        let limit = f.width().unwrap_or(3);
        f.debug_list()
            .entries(inner.visited.iter().take(limit))
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

impl LazySet {
    pub fn from_iter<I>(names: I) -> Self
    where
        I: IntoIterator<Item = Result<VertexName>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        let iter = names.into_iter();
        let inner = Inner {
            iter: Box::new(iter),
            visited: IndexSet::new(),
            state: State::Incomplete,
        };
        let hints = Hints::default();
        Self {
            inner: Arc::new(Mutex::new(inner)),
            hints,
        }
    }

    fn load_all(&self) -> Result<MutexGuard<Inner>> {
        let mut inner = self.inner.lock().unwrap();
        inner.load_more(usize::max_value(), None)?;
        Ok(inner)
    }
}

impl NameSetQuery for LazySet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        let inner = self.inner.clone();
        let iter = Iter { inner, index: 0 };
        Ok(Box::new(iter))
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        let inner = self.load_all()?;
        let iter = inner.visited.clone().into_iter().rev().map(Ok);
        Ok(Box::new(iter))
    }

    fn count(&self) -> Result<usize> {
        let inner = self.load_all()?;
        Ok(inner.visited.len())
    }

    fn last(&self) -> Result<Option<VertexName>> {
        let inner = self.load_all()?;
        Ok(inner.visited.iter().rev().nth(0).cloned())
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        if inner.visited.contains(name) {
            return Ok(true);
        } else {
            let mut loaded = Vec::new();
            loop {
                loaded.clear();
                inner.load_more(1, Some(&mut loaded))?;
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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use super::*;
    use std::collections::HashSet;

    fn lazy_set(a: &[u8]) -> LazySet {
        LazySet::from_iter(a.to_vec().into_iter().map(|b| Ok(to_name(b))))
    }

    #[test]
    fn test_lazy_basic() -> Result<()> {
        let set = lazy_set(b"\x11\x33\x22\x77\x22\x55\x11");
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
        let set = lazy_set(b"");
        assert_eq!(format!("{:?}", &set), "<lazy [] + ? more>");
        set.count().unwrap();
        assert_eq!(format!("{:?}", &set), "<lazy []>");

        let set = lazy_set(b"\x11\x33\x22");
        assert_eq!(format!("{:?}", &set), "<lazy [] + ? more>");
        let mut iter = set.iter().unwrap();
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

            let count = set.count().unwrap();
            assert!(count <= a.len());

            let set2: HashSet<_> = a.iter().cloned().collect();
            assert_eq!(count, set2.len());

            true
        }
    }
}
