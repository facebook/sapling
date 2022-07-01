/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use anyhow::Error;
pub use anyhow::Result;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("unexpected error {0}")]
    ScmDaemonUnexpectedError(String),
}
