/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

mod tree;
pub use tree::bounded_traversal;

mod dag;
pub use dag::bounded_traversal_dag;

mod stream;
pub use stream::bounded_traversal_stream;

mod common;

#[cfg(test)]
mod tests;

pub type Iter<Out> = std::iter::Flatten<std::vec::IntoIter<Option<Out>>>;
