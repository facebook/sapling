/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;
use types::hash::HexError;
use types::hash::LengthMismatchError;

#[derive(Error, Debug)]
pub enum CommitHexParseError {
    #[error(transparent)]
    LengthMismatchError(#[from] LengthMismatchError),

    #[error(transparent)]
    HexParsingError(#[from] HexError),
}

#[derive(Error, Debug)]
pub enum RevsetLookupError {
    #[error("ambiguous identifier for '{0}': {1} available")]
    AmbiguousIdentifier(String, String),

    #[error("error decoding metalog '{0}': {1}")]
    BookmarkDecodeError(String, std::io::Error),

    #[error("error parsing commit hex hash {0}: `{1}`")]
    CommitHexParseError(String, CommitHexParseError),

    #[error("unknown revision '{0}'")]
    RevsetNotFound(String),
}
