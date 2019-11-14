/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::path::PathBuf;
use std::str;

use failure::Fail;

/// The error type for parsing config files.
#[derive(Fail, Debug)]
pub enum Error {
    /// Unable to convert to a type.
    #[fail(display = "{}", _0)]
    Convert(String),

    /// Unable to parse a file due to syntax.
    #[fail(display = "{:?}:\n{}", _0, _1)]
    Parse(PathBuf, String),

    /// Unable to read a file due to IO errors.
    #[fail(display = "{:?}: {}", _0, _1)]
    Io(PathBuf, #[cause] io::Error),

    /// Config file contains invalid UTF-8.
    #[fail(display = "{:?}: {}", _0, _1)]
    Utf8(PathBuf, #[cause] str::Utf8Error),
}
