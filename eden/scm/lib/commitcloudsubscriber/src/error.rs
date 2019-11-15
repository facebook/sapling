/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use lazy_static::lazy_static;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Commit Cloud `hg cloud sync` error: {0}")]
    CommitCloudHgCloudSyncError(String),
    #[error("Commit Cloud config error: {0}")]
    CommitCloudConfigError(&'static str),
    #[error("Unexpected error: {0}")]
    CommitCloudUnexpectedError(String),
}

use regex::Regex;
lazy_static! {
    static ref RE: Regex = Regex::new(r"[\&\?]?access_token=\b\w+\b").unwrap();
}

// This error is outside the enum ErrorKind because of custom filter
// It seems #[fail(display = )] doesn't support arbitrary expressions

#[derive(Error, Debug)]
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
