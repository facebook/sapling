/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Define common EdenFS errors

use std::path::PathBuf;
use std::result::Result as StdResult;

use thiserror::Error;

pub type ExitCode = i32;
pub type Result<T, E = EdenFsError> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum EdenFsError {
    #[error("Timed out when connecting to EdenFS daemon: {0:?}")]
    ThriftConnectionTimeout(PathBuf),

    #[error("IO error when connecting to EdenFS daemon: {0:?}")]
    ThriftIoError(#[source] std::io::Error),

    #[error("Error when loading configurations: {0}")]
    ConfigurationError(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub trait ResultExt<T> {
    /// Convert any error in a `Result` type into [`EdenFsError`]. Use this when ?-operator can't
    /// automatically infer the type.
    ///
    /// Note: This method will unconditionally convert everything into [`EdenFsError::Other`]
    /// variant even if there is a better match.
    fn from_err(self) -> StdResult<T, EdenFsError>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ResultExt<T> for StdResult<T, E> {
    fn from_err(self) -> StdResult<T, EdenFsError> {
        self.map_err(|e| EdenFsError::Other(e.into()))
    }
}
