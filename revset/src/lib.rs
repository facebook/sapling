// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate asyncmemo;
#[macro_use]
extern crate error_chain;
extern crate futures;
#[macro_use]
extern crate maplit;
extern crate mercurial_types;
extern crate repoinfo;

use futures::stream::Stream;
use mercurial_types::NodeHash;

mod setcommon;

mod intersectnodestream;
pub use intersectnodestream::IntersectNodeStream;

mod unionnodestream;
pub use unionnodestream::UnionNodeStream;

mod singlenodehash;
pub use singlenodehash::SingleNodeHash;

mod setdifferencenodestream;
pub use setdifferencenodestream::SetDifferenceNodeStream;

pub mod errors;

pub type NodeStream = Stream<Item = NodeHash, Error = errors::Error> + 'static;

mod validation;
pub use validation::ValidateNodeStream;

mod ancestors;
pub use ancestors::{common_ancestors, greatest_common_ancestor, AncestorsNodeStream};

mod range;
pub use range::RangeNodeStream;

#[cfg(test)]
extern crate ascii;
#[cfg(test)]
extern crate blobrepo;
#[cfg(test)]
extern crate branch_even;
#[cfg(test)]
extern crate branch_uneven;
#[cfg(test)]
extern crate branch_wide;
#[cfg(test)]
extern crate linear;
#[cfg(test)]
extern crate merge_even;
#[cfg(test)]
extern crate merge_uneven;
#[cfg(test)]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;
#[cfg(test)]
extern crate unshared_merge_even;
#[cfg(test)]
extern crate unshared_merge_uneven;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod quickchecks;
