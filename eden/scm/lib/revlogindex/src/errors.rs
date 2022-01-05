/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::io;

use dag::errors::NotFoundError;
use dag::Id;
use dag::Vertex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RevlogIndexError {
    #[error(transparent)]
    CommitNotFound(#[from] CommitNotFound),

    #[error(transparent)]
    RevNotFound(#[from] RevNotFound),

    #[error("ambiguous prefix")]
    AmbiguousPrefix,

    // Collapse different kinds of corruption into one variant.
    // This helps keeping the enum sane.
    #[error(transparent)]
    Corruption(Box<CorruptionError>),

    #[error("unsupported: {0}")]
    Unsupported(String),
}

#[derive(Debug, Error)]
pub enum CorruptionError {
    #[error("{0}")]
    Generic(String),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    RadixTree(#[from] radixbuf::Error),

    #[error(transparent)]
    Lz4(#[from] lz4_pyframe::Error),

    #[error(transparent)]
    IndexedLog(#[from] indexedlog::Error),
}

impl From<radixbuf::Error> for RevlogIndexError {
    fn from(err: radixbuf::Error) -> Self {
        match err {
            radixbuf::Error::AmbiguousPrefix => Self::AmbiguousPrefix,
            _ => Self::Corruption(Box::new(err.into())),
        }
    }
}

impl From<lz4_pyframe::Error> for RevlogIndexError {
    fn from(err: lz4_pyframe::Error) -> Self {
        Self::Corruption(Box::new(err.into()))
    }
}

impl From<indexedlog::Error> for RevlogIndexError {
    fn from(err: indexedlog::Error) -> Self {
        Self::Corruption(Box::new(err.into()))
    }
}

// Currently, consider io::Error as a corruption error in this crate.
impl From<io::Error> for RevlogIndexError {
    fn from(err: io::Error) -> Self {
        Self::Corruption(Box::new(err.into()))
    }
}

pub fn corruption<T>(s: impl ToString) -> crate::Result<T> {
    Err(RevlogIndexError::Corruption(Box::new(
        CorruptionError::Generic(s.to_string()),
    )))
}

pub fn unsupported<T>(s: impl ToString) -> crate::Result<T> {
    Err(RevlogIndexError::Unsupported(s.to_string()))
}

impl From<CorruptionError> for RevlogIndexError {
    fn from(err: CorruptionError) -> RevlogIndexError {
        RevlogIndexError::Corruption(Box::new(err))
    }
}

#[derive(Debug)]
pub struct CommitNotFound(pub Vertex);

#[derive(Debug)]
pub struct RevNotFound(pub u32);

impl fmt::Display for CommitNotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "00changelog.i@{:.8?}: not found", &self.0)
    }
}

impl std::error::Error for CommitNotFound {}

impl fmt::Display for RevNotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "00changelog.i: rev {:.8?} not found", &self.0)
    }
}

impl std::error::Error for RevNotFound {}

impl From<RevlogIndexError> for dag::Error {
    fn from(err: RevlogIndexError) -> dag::Error {
        use dag::errors::BackendError;
        use RevlogIndexError as R;

        match err {
            R::CommitNotFound(CommitNotFound(vertex)) => vertex.not_found_error(),
            R::RevNotFound(RevNotFound(id)) => Id(id as _).not_found_error(),
            R::AmbiguousPrefix => {
                dag::Error::Bug("AmbiguousPrefix should not be translated".into())
            }
            R::Corruption(err) => match *err {
                CorruptionError::Generic(message) => BackendError::Generic(message),
                CorruptionError::Io(e) => BackendError::Io(e),
                CorruptionError::IndexedLog(e) => BackendError::IndexedLog(e),
                CorruptionError::RadixTree(e) => BackendError::Other(e.into()),
                CorruptionError::Lz4(e) => BackendError::Other(e.into()),
            }
            .into(),
            R::Unsupported(message) => dag::Error::Programming(message),
        }
    }
}
