/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mononoke_types::RepositoryId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("connection does not start with preamble")]
    NoConnectionPreamble,
    #[error("connection error while reading preamble")]
    ConnectionError,
    #[error("connection does not have a client certificate")]
    ConnectionNoClientCertificate,
    #[error("Unauthorized access, permission denied")]
    AuthorizationFailed,
    #[error("Failed to create AclChecker for tier {0}")]
    AclCheckerCreationFailed(String),
    #[error("Unexpected identity type {0}")]
    UnexpectedIdentityType(String),
    #[error("Large repo not found: {0}")]
    LargeRepoNotFound(RepositoryId),
}
