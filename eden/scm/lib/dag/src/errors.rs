/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;

use thiserror::Error;

use crate::Group;
use crate::Id;
use crate::VertexName;

/// Error used by the Dag crate.
#[derive(Debug, Error)]
pub enum DagError {
    /// A vertex name cannot be found.
    #[error("{0:?} cannot be found")]
    VertexNotFound(VertexName),

    /// An Id cannot be found.
    #[error("{0:?} cannot be found")]
    IdNotFound(Id),

    /// A fast path cannot be used.
    #[error("NeedSlowPath: {0}")]
    NeedSlowPath(String),

    /// Callsite does something wrong. For example, a "parent function" does not
    /// return reproducible results for a same vertex if called twice.
    #[error("ProgrammingError: {0}")]
    Programming(String),

    /// Logic error in this crate. A bug in this crate or the backend data.
    #[error("bug: {0}")]
    Bug(String),

    /// The backend (ex. filesystem) cannot fulfill the request somehow.
    #[error(transparent)]
    Backend(Box<BackendError>),

    /// No space for new Ids.
    #[error("out of space for group {0:?}")]
    IdOverflow(Group),
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("{0}")]
    Generic(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[cfg(any(test, feature = "indexedlog-backend"))]
    #[error(transparent)]
    IndexedLog(#[from] indexedlog::Error),

    /// Other source of backend errors. Useful for external crates implementing
    /// traits of the `dag` crate.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<BackendError> for DagError {
    fn from(err: BackendError) -> DagError {
        DagError::Backend(Box::new(err))
    }
}

#[cfg(any(test, feature = "indexedlog-backend"))]
impl From<indexedlog::Error> for DagError {
    fn from(err: indexedlog::Error) -> DagError {
        DagError::Backend(Box::new(BackendError::from(err)))
    }
}

impl From<io::Error> for DagError {
    fn from(err: io::Error) -> DagError {
        DagError::Backend(Box::new(BackendError::from(err)))
    }
}

/// Quick way to return a `BackendError::Generic` error.
pub fn bug<T>(message: impl ToString) -> crate::Result<T> {
    Err(DagError::Bug(message.to_string()))
}

/// Quick way to return a `Programming` error.
pub fn programming<T>(message: impl ToString) -> crate::Result<T> {
    Err(DagError::Programming(message.to_string()))
}

pub trait NotFoundError {
    fn not_found_error(&self) -> DagError;

    fn not_found<T>(&self) -> crate::Result<T> {
        Err(self.not_found_error())
    }
}

impl NotFoundError for Id {
    fn not_found_error(&self) -> DagError {
        ::fail::fail_point!("dag-not-found-id");
        DagError::IdNotFound(self.clone())
    }
}

impl NotFoundError for VertexName {
    fn not_found_error(&self) -> DagError {
        ::fail::fail_point!("dag-not-found-vertex");
        DagError::VertexNotFound(self.clone())
    }
}
