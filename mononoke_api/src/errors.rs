/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::backtrace::Backtrace;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use anyhow::Error;
use thiserror::Error;

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
impl_into_thrift_error!(service::RepoCreateCommitExn);
impl_into_thrift_error!(service::CommitFileDiffsExn);
impl_into_thrift_error!(service::CommitLookupExn);
impl_into_thrift_error!(service::CommitInfoExn);
impl_into_thrift_error!(service::CommitCompareExn);
impl_into_thrift_error!(service::CommitIsAncestorOfExn);
impl_into_thrift_error!(service::CommitFindFilesExn);
impl_into_thrift_error!(service::CommitPathInfoExn);
impl_into_thrift_error!(service::CommitPathBlameExn);
impl_into_thrift_error!(service::TreeListExn);
impl_into_thrift_error!(service::FileExistsExn);
impl_into_thrift_error!(service::FileInfoExn);
impl_into_thrift_error!(service::FileContentChunkExn);
impl_into_thrift_error!(service::CommitLookupXrepoExn);
