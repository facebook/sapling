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
mod iddag;
mod iddagstore;
pub mod idmap;
pub mod namedag;
pub mod nameset;
pub mod protocol;
mod segment;
pub mod spanset;

pub use id::{Group, Id, VertexName};
pub use iddag::IdDag;
pub use idmap::IdMap;
pub use namedag::NameDag;
pub use nameset::NameSet;
pub use spanset::SpanSet;

pub type Level = u8;
pub type InProcessIdDag = IdDag<iddagstore::InProcessStore>;

#[cfg(test)]
mod tests;
