/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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

impl ForeignError for anyhow::Error {}
impl ForeignError for indexedlog::Error {}
impl ForeignError for mincode::Error {}
impl ForeignError for std::io::Error {}
impl ForeignError for std::str::Utf8Error {}
impl ForeignError for std::num::ParseIntError {}
impl ForeignError for zstore::Error {}
impl ForeignError for types::hash::LengthMismatchError {}
impl ForeignError for String {}
