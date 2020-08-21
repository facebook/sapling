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

mod default_impl;
pub mod errors;
mod fmt;
pub mod id;
mod iddag;
mod iddagstore;
pub mod idmap;
mod inversedag;
pub mod namedag;
pub mod nameset;
pub mod ops;
pub mod protocol;
pub mod render;
mod segment;
pub mod spanset;
pub mod utils;

pub use id::{Group, Id, VertexName};
pub use iddag::IdDag;
pub use idmap::IdMap;
pub use inversedag::InverseDag;
pub use namedag::NameDag;
pub use nameset::NameSet;
pub use ops::DagAlgorithm;
pub use spanset::SpanSet;

pub type Level = u8;
pub type InProcessIdDag = IdDag<iddagstore::InProcessStore>;

// Short aliases for main public types.
pub type Dag = NameDag;
pub type Set = NameSet;
pub type IdSet = SpanSet;
pub use namedag::MemNameDag as MemDag;
pub use nameset::NameIter as SetIter;
pub type Vertex = VertexName;

pub mod tests;

pub use errors::DagError as Error;
pub type Result<T> = std::result::Result<T, Error>;
