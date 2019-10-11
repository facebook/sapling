/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use futures_ext::BoxStream;
use mononoke_types::ChangesetId;

mod setcommon;

mod intersectnodestream;
pub use crate::intersectnodestream::IntersectNodeStream;

mod unionnodestream;
pub use crate::unionnodestream::UnionNodeStream;

mod setdifferencenodestream;
pub use crate::setdifferencenodestream::SetDifferenceNodeStream;

pub mod errors;
pub use crate::errors::{Error, ErrorKind};

pub type BonsaiNodeStream = BoxStream<ChangesetId, errors::Error>;

mod validation;
pub use crate::validation::ValidateNodeStream;

mod ancestors;
pub use crate::ancestors::{common_ancestors, greatest_common_ancestor, AncestorsNodeStream};

mod ancestorscombinators;
pub use crate::ancestorscombinators::DifferenceOfUnionsOfAncestorsNodeStream;

mod range;
pub use crate::range::RangeNodeStream;

use uniqueheap::UniqueHeap;

pub use crate::test::*;

#[cfg(test)]
mod test {
    pub use ascii;
    pub use async_unit;
    pub use quickcheck;

    pub use fixtures;
}
#[cfg(not(test))]
mod test {}
#[cfg(test)]
mod quickchecks;
#[cfg(test)]
mod tests;
