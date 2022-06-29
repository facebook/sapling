/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod multiplex;
#[cfg(test)]
mod test;

pub use multiplex::{MultiplexQuorum, WalMultiplexedBlobstore};
