// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Errors used by the crate

use failure::Fail;
use std::borrow::Cow;
use std::path::Path;

#[derive(Fail, Debug)]
#[fail(display = "{:?}: range {}..{} failed checksum check", path, start, end)]
pub struct ChecksumError {
    pub path: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Fail, Debug)]
#[fail(display = "{}: {}", _0, _1)]
pub struct PathDataError(pub String, pub Cow<'static, str>);

#[derive(Fail, Debug)]
#[fail(display = "ProgrammingError: {}", _0)]
pub struct ProgrammingError(pub Cow<'static, str>);

#[derive(Fail, Debug)]
#[fail(display = "DataError: {}", _0)]
pub struct DataError(pub Cow<'static, str>);

#[derive(Fail, Debug)]
#[fail(display = "ParameterError: {}", _0)]
pub struct ParameterError(pub Cow<'static, str>);

#[inline(never)]
pub(crate) fn parameter_error(msg: impl Into<Cow<'static, str>>) -> failure::Error {
    ParameterError(msg.into()).into()
}

#[inline(never)]
pub(crate) fn data_error(msg: impl Into<Cow<'static, str>>) -> failure::Error {
    DataError(msg.into()).into()
}

#[inline(never)]
pub(crate) fn path_data_error(
    path: impl AsRef<Path>,
    msg: impl Into<Cow<'static, str>>,
) -> failure::Error {
    PathDataError(path.as_ref().to_string_lossy().to_string(), msg.into()).into()
}
