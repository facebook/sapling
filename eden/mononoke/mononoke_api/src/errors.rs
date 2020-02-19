/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::LoadableError;
use derived_data::DeriveError;
use std::backtrace::Backtrace;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use anyhow::Error;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct InternalError(Arc<Error>);

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Error> for InternalError {
    fn from(error: Error) -> Self {
        Self(Arc::new(error))
    }
}

impl StdError for InternalError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&**self.0)
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        Some(self.0.backtrace())
    }
}

#[derive(Clone, Debug, Error)]
pub enum MononokeError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("permission denied: {mode} access not permitted for {identities}")]
    PermissionDenied {
        mode: &'static str,
        identities: String,
    },
    #[error("not available: {0}")]
    NotAvailable(String),
    #[error("internal error: {0}")]
    InternalError(#[source] InternalError),
}

impl From<Error> for MononokeError {
    fn from(e: Error) -> Self {
        MononokeError::InternalError(InternalError(Arc::new(e)))
    }
}

impl From<Infallible> for MononokeError {
    fn from(_i: Infallible) -> Self {
        unreachable!()
    }
}

impl From<LoadableError> for MononokeError {
    fn from(e: LoadableError) -> Self {
        MononokeError::InternalError(InternalError(Arc::new(e.into())))
    }
}

impl From<DeriveError> for MononokeError {
    fn from(e: DeriveError) -> Self {
        match e {
            e @ DeriveError::Disabled(_, _) => MononokeError::NotAvailable(e.to_string()),
            DeriveError::Error(e) => MononokeError::from(e),
        }
    }
}
