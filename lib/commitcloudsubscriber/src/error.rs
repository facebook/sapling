/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use failure::Fail;
use lazy_static::lazy_static;
use std::fmt;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Commit Cloud `hg cloud sync` error: {}", _0)]
    CommitCloudHgCloudSyncError(String),
    #[fail(display = "Commit Cloud config error: {}", _0)]
    CommitCloudConfigError(&'static str),
    #[fail(display = "Unexpected error: {}", _0)]
    CommitCloudUnexpectedError(String),
}

use regex::Regex;
lazy_static! {
    static ref RE: Regex = Regex::new(r"[\&\?]?access_token=\b\w+\b").unwrap();
}

// This error is outside the enum ErrorKind because of custom filter
// It seems #[fail(display = )] doesn't support arbitrary expressions

#[derive(Fail, Debug)]
pub struct CommitCloudHttpError(pub String);
impl fmt::Display for CommitCloudHttpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Commit Cloud EventSource HTTP error: {}",
            // remove any token
            RE.replace_all(&self.0, "")
        )
    }
}
