// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

pub use failure::Error;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "unexpected error {}", _0)]
    ScmDaemonUnexpectedError(String),
}

pub type Result<T> = ::std::result::Result<T, Error>;
