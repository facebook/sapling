// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use crate::failure::{Error, Result, ResultExt};

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
