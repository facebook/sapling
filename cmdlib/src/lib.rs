// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![deny(warnings)]

extern crate ascii;
extern crate bytes;
extern crate cachelib;
extern crate clap;
#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate sloggers;

extern crate slog_glog_fmt;

extern crate blobrepo;
extern crate bookmarks;
extern crate hooks;
extern crate mercurial;
extern crate mercurial_types;
extern crate metaconfig;
extern crate panichandler;
extern crate repo_client;
extern crate scuba_ext;

pub mod args;
pub mod blobimport_lib;
