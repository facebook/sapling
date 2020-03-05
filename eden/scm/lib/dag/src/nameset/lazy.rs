/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{r#static::IterRev, NameIter, NameSetQuery};
use crate::VertexName;
use anyhow::{anyhow, bail, Result};
use indexmap::IndexSet;
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// A set backed by a lazy iterator of names.
pub struct LazySet {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    iter: Box<dyn Iterator<Item = Result<VertexName>> + Send + Sync>,
    visited: IndexSet<VertexName>,
    state: State,
}

impl Inner {
    fn load_more(&mut self, n: usize, mut out: Option<&mut Vec<VertexName>>) -> Result<()> {
        if self.is_completed()? {
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

    fn is_completed(&self) -> Result<bool> {
        match self.state {
            State::Error => bail!("Iteration has errored out"),
            State::Complete => Ok(true),
            State::Incomplete => Ok(false),
        }
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
                State::Error => break Some(Err(anyhow!("Iteration has errored out"))),
                State::Complete if inner.visited.len() <= self.index => break None,
                State::Complete | State::Incomplete => {
                    match inner.visited.get_index(self.index) {
                        Some(value) => break Some(Ok(value.clone())),
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

impl NameIter for Iter {}

impl fmt::Debug for LazySet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<lazy>")
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
        Self {
            inner: Arc::new(Mutex::new(inner)),
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
        let iter: IterRev = inner.visited.clone().into_iter().rev().map(Ok);
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
            while !inner.is_completed()? {
                loaded.clear();
                inner.load_more(1, Some(&mut loaded))?;
                debug_assert!(loaded.len() <= 1);
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
}
