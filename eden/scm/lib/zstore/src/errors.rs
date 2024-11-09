/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Minimalistic error handling that just converts all errors to strings.
//
// The error type implements `std::error::Error` so it does not enforce
// callsites to use a particular library like failure or anyhow.

use std::fmt;

#[derive(Debug)]
pub struct Error(pub(crate) String);

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {}

pub trait ForeignError: ToString {}

impl<T: ForeignError> From<T> for Error {
    fn from(err: T) -> Error {
        Error(err.to_string())
    }
}

impl ForeignError for indexedlog::Error {}
impl ForeignError for mincode::Error {}
impl ForeignError for std::io::Error {}
