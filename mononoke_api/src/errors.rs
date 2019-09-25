// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;
use std::sync::Arc;

use failure::{Backtrace, Error, Fail};

use source_control::services::source_control_service as service;
use source_control::types as thrift;

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

impl Fail for InternalError {
    fn cause(&self) -> Option<&dyn Fail> {
        Some(self.0.as_fail())
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        Some(self.0.backtrace())
    }
}

#[derive(Clone, Debug, Fail)]
pub enum MononokeError {
    #[fail(display = "invalid request: {}", _0)]
    InvalidRequest(String),
    #[fail(display = "internal error: {}", _0)]
    InternalError(#[fail(cause)] InternalError),
}

impl From<Error> for MononokeError {
    fn from(e: Error) -> Self {
        MononokeError::InternalError(InternalError(Arc::new(e)))
    }
}

macro_rules! impl_into_thrift_error(
    ($t:ty) => {
        impl From<MononokeError> for $t {
            fn from(e: MononokeError) -> Self {
                match e {
                    MononokeError::InvalidRequest(reason) => thrift::RequestError {
                        kind: thrift::RequestErrorKind::INVALID_REQUEST,
                        reason,
                    }
                    .into(),
                    MononokeError::InternalError(error) => thrift::InternalError {
                        reason: error.to_string(),
                        backtrace: error.backtrace().map(ToString::to_string),
                    }
                    .into(),
                }
            }
        }
    }
);

// Implement From<MononokeError> for source control service exceptions. This allows using ? on
// MononokeError and have it turn into the right exception. When adding a new error to source
// control, add it here to get this behavior for free.
impl_into_thrift_error!(service::RepoResolveBookmarkExn);
impl_into_thrift_error!(service::RepoListBookmarksExn);
impl_into_thrift_error!(service::CommitLookupExn);
impl_into_thrift_error!(service::CommitInfoExn);
impl_into_thrift_error!(service::CommitIsAncestorOfExn);
impl_into_thrift_error!(service::CommitPathInfoExn);
impl_into_thrift_error!(service::TreeListExn);
impl_into_thrift_error!(service::FileExistsExn);
