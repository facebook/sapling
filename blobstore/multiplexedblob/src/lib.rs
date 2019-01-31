// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate cloned;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate futures_stats;
extern crate lazy_static;
extern crate scuba;
extern crate time_ext;
extern crate tokio;

extern crate blobstore;
extern crate blobstore_sync_queue;
extern crate context;
extern crate metaconfig_types;
extern crate mononoke_types;

#[cfg(test)]
extern crate async_unit;

pub mod base;
pub mod queue;

pub use queue::MultiplexedBlobstore;

#[cfg(test)]
mod test;
