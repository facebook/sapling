// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![deny(warnings)]

extern crate clap;
#[macro_use]
extern crate slog;

extern crate slog_glog_fmt;

extern crate blobrepo;
extern crate mercurial_types;

pub mod args;
