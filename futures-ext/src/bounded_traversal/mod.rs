// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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
