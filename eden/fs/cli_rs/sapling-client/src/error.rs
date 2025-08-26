/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::io;

use thiserror::Error;

pub type ExitCode = i32;
pub type Result<T, E = SaplingError> = std::result::Result<T, E>;

#[derive(Clone, Debug)]
pub struct SaplingIoError {
    pub error_kind: io::ErrorKind,
    pub message: String,
}

impl fmt::Display for SaplingIoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "error_kind: {}, message: {}",
            self.error_kind, self.message
        )
    }
}

impl From<io::Error> for SaplingIoError {
    fn from(from: io::Error) -> Self {
        SaplingIoError {
            error_kind: from.kind(),
            message: from.to_string(),
        }
    }
}

#[derive(Clone, Debug, Error)]
pub enum SaplingError {
    #[error("The Sapling command failed with: {0}.")]
    IO(SaplingIoError),
    #[error("The Sapling command failed with: {0}.")]
    Utf8Error(std::string::FromUtf8Error),
    #[error("The Sapling command failed with: {0}.")]
    FloatError(std::num::ParseFloatError),
    #[error("The Sapling command failed with: {0}.")]
    IntError(std::num::ParseIntError),
    #[error("The Sapling command failed with: {0}.")]
    Other(String),
}

macro_rules! impl_from_error {
    ($from:ty, $to:expr) => {
        impl From<$from> for SaplingError {
            fn from(from: $from) -> Self {
                $to(from)
            }
        }
    };
}

impl_from_error!(std::string::FromUtf8Error, SaplingError::Utf8Error);
impl_from_error!(std::num::ParseFloatError, SaplingError::FloatError);
impl_from_error!(std::num::ParseIntError, SaplingError::IntError);

impl From<io::Error> for SaplingError {
    fn from(from: io::Error) -> Self {
        SaplingError::IO(from.into())
    }
}

impl From<anyhow::Error> for SaplingError {
    fn from(from: anyhow::Error) -> Self {
        SaplingError::Other(from.to_string())
    }
}
