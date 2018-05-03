// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure::{Error, Result, ResultExt};

use mercurial_types::RepoPath;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "failed to initialize server: {}", _0)] Initialization(&'static str),
    #[fail(display = "internal error: file {} copied from directory {}", _0, _1)]
    InconsistenCopyInfo(RepoPath, RepoPath),
    #[fail(display = "connection does not start with preamble")] NoConnectionPreamble,
    #[fail(display = "connection error while reading preamble")] ConnectionError,
    #[fail(display = "incorrect reponame: {}", _0)] IncorrectRepoName(String),
}
