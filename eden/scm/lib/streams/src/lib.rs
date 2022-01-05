/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `streams` provides some generic streams that can be useful in other places.
//! - `HybridStream` provides a way to use local data (with a single point get
//!   API) and remote data (with an unordered batch get API) to resolve a stream
//!   of input into a stream of output.
//! - `SelectDrop` provides a version of `futures::stream::Select` which drops
//!   each of the combined streams after it terminates. This is useful for
//!   preventing deadlocks when one stream is waiting on another to be dropped
//!   to complete.

mod hybrid;
mod select_drop;

pub use hybrid::HybridResolver;
pub use hybrid::HybridStream;
pub use select_drop::select_drop;
pub use select_drop::SelectDrop;
