/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

//! # dag
//!
//! Building blocks for the commit graph used by source control.

pub mod id;
pub mod idmap;
pub mod nameddag;
pub mod protocol;
pub mod segment;
pub mod spanset;

pub use id::{Group, Id, VertexName};
pub use idmap::IdMap;
pub use nameddag::NamedDag;
pub use segment::IdDag;

#[cfg(test)]
mod tests;
