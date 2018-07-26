// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

//! State for a single source control Repo

extern crate bytes;
#[macro_use]
extern crate failure_ext as failure;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate futures_stats;
extern crate itertools;
#[macro_use]
extern crate lazy_static;
extern crate pylz4;
extern crate rand;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats;
extern crate time_ext;
#[macro_use]
extern crate tracing;

extern crate blobrepo;
extern crate bundle2_resolver;
extern crate filenodes;
extern crate hgproto;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
extern crate metaconfig;
extern crate revset;
extern crate scuba_ext;

mod client;
mod errors;
mod mononoke_repo;

pub use client::RepoClient;
pub use mononoke_repo::MononokeRepo;
