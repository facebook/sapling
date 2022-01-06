/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![deny(warnings)]

pub mod errors;
pub use crate::errors::ErrorKind;

mod index;
pub use crate::index::{LeastCommonAncestorsHint, NodeFrontier, ReachabilityIndex};
