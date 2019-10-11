/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure::Fail;
pub use failure_ext::{Error, Result, ResultExt};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "connection does not start with preamble")]
    NoConnectionPreamble,
    #[fail(display = "connection error while reading preamble")]
    ConnectionError,
    #[fail(display = "connection does not have a client certificate")]
    ConnectionNoClientCertificate,
    #[fail(display = "Unauthorized access, permission denied")]
    AuthorizationFailed,
    #[fail(display = "Failed to create AclChecker for tier {}", _0)]
    AclCheckerCreationFailed(String),
    #[fail(display = "Unexpected identity type {}", _0)]
    UnexpectedIdentityType(String),
}
