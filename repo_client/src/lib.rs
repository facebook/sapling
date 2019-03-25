// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

//! State for a single source control Repo

extern crate bytes;
#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate fbwhoami;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate futures_stats;
extern crate itertools;
#[macro_use]
extern crate maplit;
extern crate percent_encoding;
extern crate rand;
extern crate scribe;
extern crate scribe_cxx;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats;
extern crate time_ext;
#[macro_use]
extern crate tracing;

extern crate blobrepo;
extern crate blobstore;
extern crate bookmarks;
extern crate bundle2_resolver;
extern crate context;
extern crate filenodes;
extern crate hgproto;
extern crate hooks;
extern crate lz4_pyframe;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
extern crate metaconfig_types;
extern crate mononoke_types;
extern crate phases;
extern crate reachabilityindex;
extern crate remotefilelog;
extern crate revset;
extern crate scuba_ext;
#[macro_use]
extern crate sql;
extern crate sql_ext;
extern crate streaming_clone;
extern crate tokio;

mod client;
mod errors;
mod mononoke_repo;
mod read_write;

pub use client::RepoClient;
pub use mononoke_repo::{streaming_clone, MononokeRepo};
pub use read_write::RepoReadWriteFetcher;
pub use streaming_clone::SqlStreamingChunksFetcher;
