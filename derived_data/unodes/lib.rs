// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use failure_ext::Fail;

mod derive;
mod mapping;

pub use mapping::{RootUnodeManifestId, RootUnodeManifestMapping};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Invalid bonsai changeset: {}", _0)]
    InvalidBonsai(String),
}
