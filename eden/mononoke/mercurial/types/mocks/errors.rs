/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("invalid manifest description: {0}")]
    InvalidManifestDescription(String),
    #[error("invalid path map: {0}")]
    InvalidPathMap(String),
    #[error("invalid directory hash map: {0}")]
    InvalidDirectoryHashes(String),
}
