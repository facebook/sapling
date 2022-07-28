/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Read the documentation of [bounded_traversal](crate::bounded_traversal),
//! [bounded_traversal_dag](crate::bounded_traversal_dag) and
//! [bounded_traversal_stream](crate::bounded_traversal_stream)

#[macro_use]
mod error;
pub use error::BoundedTraversalError;

mod tree;
pub use tree::bounded_traversal;

mod dag;
pub use dag::bounded_traversal_dag;

mod stream;
pub use stream::bounded_traversal_stream;
pub use stream::limited_by_key_shardable;

mod ordered_stream;
pub use ordered_stream::bounded_traversal_limited_ordered_stream;
pub use ordered_stream::bounded_traversal_ordered_stream;

mod common;
pub use common::OrderedTraversal;

#[cfg(test)]
mod tests;

/// A type used frequently in fold-like invocations inside this module
pub type Iter<Out> = std::iter::Flatten<std::vec::IntoIter<Option<Out>>>;
