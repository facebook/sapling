// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate asyncmemo;
extern crate blobrepo;
#[macro_use]
extern crate failure_ext as failure;
#[macro_use]
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate maplit;
extern crate mercurial_types;
extern crate repoinfo;

use futures::stream::Stream;
use mercurial_types::DNodeHash;

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
pub use errors::{Error, ErrorKind};

pub type NodeStream = Stream<Item = DNodeHash, Error = errors::Error> + Send + 'static;

mod validation;
pub use validation::ValidateNodeStream;

mod ancestors;
pub use ancestors::{common_ancestors, greatest_common_ancestor, AncestorsNodeStream};

mod ancestorscombinators;
pub use ancestorscombinators::DifferenceOfUnionsOfAncestorsNodeStream;

mod range;
pub use range::RangeNodeStream;

pub use test::*;
#[cfg(test)]
mod test {
    pub extern crate ascii;
    pub extern crate async_unit;
    pub extern crate quickcheck;
    pub extern crate rand;

    pub extern crate branch_even;
    pub extern crate branch_uneven;
    pub extern crate branch_wide;
    pub extern crate linear;
    pub extern crate merge_even;
    pub extern crate merge_uneven;
    pub extern crate unshared_merge_even;
    pub extern crate unshared_merge_uneven;
}
#[cfg(not(test))]
mod test {}
#[cfg(test)]
mod tests;
#[cfg(test)]
mod quickchecks;
