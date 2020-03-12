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

/// A set that is marked as topologically sorted.
///
/// Useful for [`LazySet`] and [`StaticSet`].
pub struct SortedSet(pub(crate) NameSet);

impl SortedSet {
    pub fn from_set(set: NameSet) -> Self {
        Self(set)
    }
}

impl NameSetQuery for SortedSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        self.0.iter()
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        self.0.iter_rev()
    }

    fn count(&self) -> Result<usize> {
        self.0.count()
    }

    fn is_empty(&self) -> Result<bool> {
        self.0.is_empty()
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        self.0.contains(name)
    }

    fn is_topo_sorted(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl fmt::Debug for SortedSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<sorted {:?}>", &self.0)
    }
}
