// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure_ext::{Error, Fail, Result, ResultExt};

use mercurial_types::{HgNodeHash, RepoPath};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(
        display = "Data corruption for {}: expected {}, actual {}!",
        _0, _1, _2
    )]
    DataCorruption {
        path: RepoPath,
        expected: HgNodeHash,
        actual: HgNodeHash,
    },
    #[fail(display = "Request {} was throttled", _0)]
    RequestThrottled { request_name: String },
}
