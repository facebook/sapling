// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Code for operations on blobrepos that are useful but not essential.

#![deny(warnings)]

extern crate chashmap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate slog;
extern crate tokio;

extern crate futures_ext;

extern crate blobrepo;
extern crate bonsai_utils;
extern crate mercurial_types;

mod changeset;
mod errors;

pub use changeset::{visit_changesets, ChangesetVisitor};
pub use errors::ErrorKind;
