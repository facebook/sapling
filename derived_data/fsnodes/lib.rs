// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use failure_ext::Fail;
use mononoke_types::{ContentId, FsnodeId};

mod derive;
mod mapping;

pub use mapping::{RootFsnodeId, RootFsnodeMapping};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Invalid bonsai changeset: {}", _0)]
    InvalidBonsai(String),
    #[fail(display = "Missing content: {}", _0)]
    MissingContent(ContentId),
    #[fail(display = "Missing fsnode parent: {}", _0)]
    MissingParent(FsnodeId),
    #[fail(display = "Missing fsnode subentry for '{}': {}", _0, _1)]
    MissingSubentry(String, FsnodeId),
}
