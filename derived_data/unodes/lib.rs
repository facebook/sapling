/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

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
