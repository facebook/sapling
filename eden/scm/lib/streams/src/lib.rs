/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `streams` provides some generic streams that can be useful in other places.
//! - `HybridStream` provides a way to use local data (with a single point get
//!   API) and remote data (with an unordered batch get API) to resolve a stream
//!   of input into a stream of output.

mod hybrid;

pub use hybrid::HybridResolver;
pub use hybrid::HybridStream;
