// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(conservative_impl_trait)]

extern crate bytes;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate slog;
extern crate tokio_io;

extern crate blobrepo;
extern crate mercurial_bundles;
extern crate mercurial_types;

pub mod errors;
mod resolver;
mod wirepackparser;

pub use resolver::resolve;
