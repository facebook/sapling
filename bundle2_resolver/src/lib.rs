// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![cfg_attr(test, type_length_limit = "2097152")]

extern crate ascii;
#[cfg(test)]
extern crate async_unit;
extern crate blobstore;
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
#[allow(unused_imports)] // workaround for macro_use
#[macro_use]
extern crate lazy_static;
#[cfg(not(test))]
extern crate quickcheck;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
extern crate pushrebase;
extern crate reachabilityindex;
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
extern crate blobrepo_factory;
extern crate bonsai_utils;
extern crate bookmarks;
extern crate context;
extern crate hooks;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate metaconfig_types;
extern crate mononoke_types;
extern crate phases;
extern crate wirepack;

mod changegroup;
pub mod errors;
mod getbundle_response;
mod resolver;
mod stats;
mod upload_blobs;

pub use getbundle_response::create_getbundle_response;
pub use resolver::resolve;
