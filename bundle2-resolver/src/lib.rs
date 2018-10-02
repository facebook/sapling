// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
#[cfg(test)]
extern crate async_unit;
extern crate bytes;
#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
#[cfg(test)]
extern crate fixtures;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate futures_stats;
extern crate heapsize;
#[cfg(test)]
extern crate itertools;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate maplit;
#[cfg(not(test))]
extern crate quickcheck;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
extern crate revset;
extern crate scuba_ext;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats as stats_crate;
#[cfg(test)]
extern crate tests_utils;
extern crate tokio_io;

extern crate blobrepo;
extern crate bonsai_utils;
extern crate bookmarks;
extern crate hooks;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate metaconfig;
extern crate mononoke_types;

mod changegroup;
pub mod errors;
mod getbundle_response;
mod pushrebase;
mod resolver;
mod stats;
mod wirepackparser;
mod upload_blobs;

pub use getbundle_response::create_getbundle_response;
pub use resolver::resolve;
