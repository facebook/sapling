/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::Vertex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommitError {
    #[error(transparent)]
    Dag(#[from] dag::Error),

    #[error("hash mismatch ({0:?} != {1:?})")]
    HashMismatch(Vertex, Vertex),

    #[error("{0} is unsupported")]
    Unsupported(&'static str),
}

impl From<std::io::Error> for CommitError {
    fn from(err: std::io::Error) -> Self {
        Self::Dag(dag::errors::BackendError::from(err).into())
    }
}

impl From<anyhow::Error> for CommitError {
    fn from(err: anyhow::Error) -> Self {
        Self::Dag(dag::errors::BackendError::from(err).into())
    }
}

impl From<revlogindex::Error> for CommitError {
    fn from(err: revlogindex::Error) -> Self {
        anyhow::Error::from(err).into()
    }
}

impl From<zstore::Error> for CommitError {
    fn from(err: zstore::Error) -> Self {
        anyhow::Error::from(err).into()
    }
}

impl From<gitdag::git2::Error> for CommitError {
    fn from(err: gitdag::git2::Error) -> Self {
        anyhow::Error::from(err).into()
    }
}

impl From<metalog::Error> for CommitError {
    fn from(err: metalog::Error) -> Self {
        anyhow::Error::from(err).into()
    }
}

impl From<types::hash::LengthMismatchError> for CommitError {
    fn from(err: types::hash::LengthMismatchError) -> Self {
        anyhow::Error::from(err).into()
    }
}

pub fn test_only(name: &str) -> CommitError {
    dag::Error::Programming(format!("{} should only be used in tests", name)).into()
}
