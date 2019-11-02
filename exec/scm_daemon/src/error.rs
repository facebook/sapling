/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

pub use failure::{Error, Fail};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "unexpected error {}", _0)]
    ScmDaemonUnexpectedError(String),
}

pub type Result<T> = ::std::result::Result<T, Error>;
