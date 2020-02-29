/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{NameIter, NameSetQuery};
use crate::idmap::IdMap;
use crate::idmap::IdMapLike;
use crate::spanset::{SpanSet, SpanSetIter};
use crate::VertexName;
use anyhow::Result;
use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// A set backed by [`SpanSet`] + [`IdMap`].
/// Efficient for DAG calculation.
pub struct DagSet {
    pub(crate) spans: SpanSet,
    pub(crate) map: Arc<IdMap>,
}

struct Iter {
    iter: SpanSetIter<SpanSet>,
    map: Arc<IdMap>,
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
        self.spans.fmt(f)
    }
}

impl DagSet {
    pub(crate) fn from_spans_idmap(spans: SpanSet, map: Arc<IdMap>) -> Self {
        Self { spans, map }
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
        let map = &self.map;
        match map.find_id_by_name(name.as_ref())? {
            Some(id) => Ok(self.spans.contains(id)),
            None => Ok(false),
        }
    }

    fn is_topo_sorted(&self) -> bool {
        // SpanSet is always sorted.
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
