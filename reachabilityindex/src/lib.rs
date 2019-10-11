/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
pub mod errors;
pub use crate::errors::ErrorKind;

mod index;
pub use crate::index::{LeastCommonAncestorsHint, NodeFrontier, ReachabilityIndex};
