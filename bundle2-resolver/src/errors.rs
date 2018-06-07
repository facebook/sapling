// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure::{Error, Result, ResultExt};

use mercurial_types::HgNodeHash;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Malformed treemanifest part: {}", _0)] MalformedTreemanifestPart(String),
    #[fail(display = "Error while uploading data for changesets, hashes: {:?}", _0)]
    WhileUploadingData(Vec<HgNodeHash>),
}
