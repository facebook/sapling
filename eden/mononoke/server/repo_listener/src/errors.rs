/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::RepositoryId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("connection does not start with preamble")]
    NoConnectionPreamble,
    #[error("connection does not have a client certificate")]
    ConnectionNoClientCertificate,
    #[error("Unauthorized access, permission denied")]
    AuthorizationFailed,
    #[error("Large repo not found: {0}")]
    LargeRepoNotFound(RepositoryId),
}
