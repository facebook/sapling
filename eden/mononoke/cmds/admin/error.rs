/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubcommandError {
    #[error("SubcommandError::InvalidArgs")]
    InvalidArgs,
    #[error("SubcommandError::Error")]
    Error(#[from] Error),
}
