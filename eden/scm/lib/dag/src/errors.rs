/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Id;
use crate::VertexName;
use std::io;
use thiserror::Error;

/// Error used by the Dag crate.
#[derive(Debug, Error)]
pub enum DagError {
    /// A vertex name cannot be found.
    #[error("{0:?} cannot be found")]
    VertexNotFound(VertexName),

    /// An Id cannot be found.
    #[error("{0:?} cannot be found")]
    IdNotFound(Id),

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
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("{0}")]
    Generic(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

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

impl Id {
    pub fn not_found_error(&self) -> DagError {
        DagError::IdNotFound(self.clone())
    }
    pub fn not_found<T>(&self) -> crate::Result<T> {
        Err(self.not_found_error())
    }
}

impl VertexName {
    pub fn not_found_error(&self) -> DagError {
        DagError::VertexNotFound(self.clone())
    }
    pub fn not_found<T>(&self) -> crate::Result<T> {
        Err(self.not_found_error())
    }
}
