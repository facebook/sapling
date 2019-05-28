// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Code for operations on blobrepos that are useful but not essential.

#![deny(warnings)]

extern crate chashmap;
#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate slog;
extern crate tokio;

extern crate futures_ext;

extern crate blobrepo;
extern crate bonsai_utils;
extern crate context;
extern crate mercurial_types;
extern crate mononoke_types;

mod bonsai;
mod changeset;
mod errors;

pub use crate::bonsai::{BonsaiMFVerify, BonsaiMFVerifyDifference, BonsaiMFVerifyResult};
pub use crate::changeset::{visit_changesets, ChangesetVisitor};
pub use crate::errors::ErrorKind;

pub mod internals {
    // This shouldn't actually be public, but it needs to be because of
    // https://github.com/rust-lang/rust/issues/50865.
    // TODO: (rain1) T31595868 make apply_diff private once Rust 1.29 is released
    pub use crate::bonsai::apply_diff;
}
