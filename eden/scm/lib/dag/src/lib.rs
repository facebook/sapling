/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]
#![allow(clippy::iter_nth_zero)]

//! # dag
//!
//! Building blocks for the commit graph used by source control.

mod bsearch;
mod default_impl;
mod delegate;
pub mod errors;
mod fmt;
mod iddag;
pub mod iddagstore;
pub mod idmap;
mod locked;
pub mod namedag;
pub mod nameset;
pub mod ops;
pub mod protocol;
pub mod render;
pub mod segment;
mod spanset;
pub mod utils;
mod verlink;

pub use dag_types::clone;
pub use dag_types::id;

pub use dag_types::{CloneData, Group, Id, VertexName};
pub use iddag::IdDag;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use idmap::IdMap;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use namedag::NameDag;
pub use nameset::NameSet;
pub use ops::DagAlgorithm;
pub use segment::{FlatSegment, PreparedFlatSegments};
pub use verlink::VerLink;

pub type Level = u8;
pub type InProcessIdDag = IdDag<iddagstore::InProcessStore>;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub type OnDiskIdDag = IdDag<iddagstore::IndexedLogStore>;

// Short aliases for main public types.
#[cfg(any(test, feature = "indexedlog-backend"))]
pub type Dag = NameDag;
pub type Set = NameSet;
pub type IdSet = spanset::SpanSet;
pub type IdSetIter<T> = spanset::SpanSetIter<T>;
pub type IdSpan = spanset::Span;
pub use namedag::MemNameDag as MemDag;
pub use nameset::NameIter as SetIter;
pub type Vertex = VertexName;

#[cfg(feature = "indexedlog-backend")]
pub mod tests;

pub use errors::DagError as Error;
pub type Result<T> = std::result::Result<T, Error>;

pub use nonblocking;

#[cfg(test)]
dev_logger::init!();
