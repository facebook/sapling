/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::backtrace::Backtrace;
use std::error::Error;
use std::fmt::{self, Display};

use blobrepo_errors::ErrorKind as BlobRepoError;
use thiserror::Error;

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

impl Error for BlobRepoErrorCause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.cause {
            Some(ref error) => error.source(),
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

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("{0} not found in repo")]
    NodeNotFound(String),
    #[error("failed to fetch node generation")]
    GenerationFetchFailed(#[source] BlobRepoErrorCause),
    #[error("failed to fetch parent nodes")]
    ParentsFetchFailed(#[source] BlobRepoErrorCause),
    #[error("checking existence failed")]
    CheckExistenceFailed(String, #[source] BlobRepoErrorCause),
    #[error("Unknown field in thrift encoding")]
    UknownSkiplistThriftEncoding,
}
