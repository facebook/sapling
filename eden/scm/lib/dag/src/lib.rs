/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(dead_code)]
#![allow(clippy::iter_nth_zero, clippy::for_loops_over_fallibles)]

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
mod integrity;
pub mod namedag;
pub mod nameset;
pub mod ops;
pub mod protocol;
pub mod render;
pub mod segment;
mod spanset;
pub(crate) mod types_ext;
pub mod utils;
mod verlink;
mod vertex_options;

pub use dag_types::clone;
pub use dag_types::id;
pub use dag_types::CloneData;
pub use dag_types::Group;
pub use dag_types::Id;
pub use dag_types::Location;
pub use dag_types::VertexName;
pub use iddag::FirstAncestorConstraint;
pub use iddag::IdDag;
pub use iddag::IdDagAlgorithm;
pub use iddagstore::IdDagStore;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use idmap::IdMap;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use namedag::NameDag;
pub use namedag::NameDagBuilder;
pub use nameset::NameSet;
pub use ops::DagAlgorithm;
pub use segment::FlatSegment;
pub use segment::IdSegment;
pub use segment::PreparedFlatSegments;
pub use verlink::VerLink;
pub use vertex_options::VertexListWithOptions;
pub use vertex_options::VertexOptions;

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
pub use iddagstore::indexedlog_store::describe_indexedlog_entry;

#[cfg(feature = "indexedlog-backend")]
pub mod tests;

pub use errors::DagError as Error;
pub type Result<T> = std::result::Result<T, Error>;

// Re-export
#[cfg(feature = "indexedlog-backend")]
pub use indexedlog::Repair;
pub use nonblocking;

#[macro_export]
macro_rules! failpoint {
    ($name:literal) => {
        ::fail::fail_point!($name, |_| {
            let msg = format!("failpoint injected by FAILPOINTS: {}", $name);
            Err($crate::errors::DagError::from(
                $crate::errors::BackendError::Generic(msg),
            ))
        })
    };
}

/// Whether running inside a test.
pub(crate) fn is_testing() -> bool {
    std::env::var("TESTTMP").is_ok()
}

#[cfg(test)]
dev_logger::init!();
