/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{NameIter, NameSetQuery};
use crate::VertexName;
use anyhow::Result;
use std::any::Any;
use std::fmt;

/// A virtual set that includes everything.
///
/// Most operations are unsupported. It can only be intersected with other sets.
#[derive(Copy, Clone, Debug)]
pub struct AllSet;

#[derive(Copy, Clone, Debug)]
pub struct AllSetIterError;

impl fmt::Display for AllSetIterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "cannot iterate on AllSet")
    }
}

impl std::error::Error for AllSetIterError {}

impl AllSet {
    pub fn new() -> Self {
        Self
    }
}

impl NameSetQuery for AllSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        Err(AllSetIterError.into())
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        Err(AllSetIterError.into())
    }

    fn count(&self) -> Result<usize> {
        Err(AllSetIterError.into())
    }

    fn is_empty(&self) -> Result<bool> {
        Err(AllSetIterError.into())
    }

    fn contains(&self, _name: &VertexName) -> Result<bool> {
        Ok(true)
    }

    fn is_topo_sorted(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
