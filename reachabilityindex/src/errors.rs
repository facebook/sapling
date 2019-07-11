// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;
use std::fmt::Display;

use blobrepo_errors::ErrorKind as BlobRepoError;
use failure_ext::{Backtrace, Fail};

#[derive(Debug)]
pub struct BlobRepoErrorCause {
    cause: Option<BlobRepoError>,
}

impl BlobRepoErrorCause {
    pub fn new(cause: Option<BlobRepoError>) -> Self {
        BlobRepoErrorCause { cause }
    }
}

impl Display for BlobRepoErrorCause {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({:?})", self.cause)
    }
}

impl Fail for BlobRepoErrorCause {
    fn cause(&self) -> Option<&Fail> {
        match self.cause {
            Some(ref error) => error.cause(),
            None => None,
        }
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        match self.cause {
            Some(ref error) => error.backtrace(),
            None => None,
        }
    }
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "{} not found in repo", _0)]
    NodeNotFound(String),
    #[fail(display = "failed to fetch node generation")]
    GenerationFetchFailed(#[cause] BlobRepoErrorCause),
    #[fail(display = "failed to fetch parent nodes")]
    ParentsFetchFailed(#[cause] BlobRepoErrorCause),
    #[fail(display = "checking existence failed")]
    CheckExistenceFailed(String, #[cause] BlobRepoErrorCause),
    #[fail(display = "Unknown field in thrift encoding")]
    UknownSkiplistThriftEncoding,
}
