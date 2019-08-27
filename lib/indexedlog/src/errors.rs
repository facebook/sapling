// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Errors used by the crate

use failure::Fail;

#[derive(Fail, Debug)]
#[fail(display = "{:?}: range {}..{} failed checksum check", path, start, end)]
pub struct ChecksumError {
    pub path: String,
    pub start: u64,
    pub end: u64,
}

define_error!(
    DataError,
    "An internal assumption about data went wrong. Most likely caused by filesystem corruption."
);
define_error!(ParameterError, "Parameter provided is invalid.");

pub(crate) fn parameter_error(msg: impl AsRef<str>) -> failure::Error {
    ParameterError::from(msg.as_ref().to_string()).into()
}

pub(crate) fn data_error(msg: impl AsRef<str>) -> failure::Error {
    DataError::from(format!("data corruption: {}", msg.as_ref())).into()
}
