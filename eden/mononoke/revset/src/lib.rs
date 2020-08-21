/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use futures_ext::BoxStream;
use mononoke_types::ChangesetId;

mod setcommon;
pub use setcommon::add_generations_by_bonsai;

mod intersectnodestream;
pub use crate::intersectnodestream::IntersectNodeStream;

mod unionnodestream;
pub use crate::unionnodestream::UnionNodeStream;

mod setdifferencenodestream;
pub use crate::setdifferencenodestream::SetDifferenceNodeStream;

pub mod errors;
pub use crate::errors::ErrorKind;

pub type BonsaiNodeStream = BoxStream<ChangesetId, Error>;

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
