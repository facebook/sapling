// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

pub mod base;
pub mod queue;

pub use crate::queue::{MultiplexedBlobstore, ScrubBlobstore};

#[cfg(test)]
mod test;
