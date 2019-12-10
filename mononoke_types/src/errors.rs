/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::path::MPath;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("invalid blake2 input: {0}")]
    InvalidBlake2Input(String),
    #[error("invalid sha1 input: {0}")]
    InvalidSha1Input(String),
    #[error("invalid sha256 input: {0}")]
    InvalidSha256Input(String),
    #[error("invalid git sha1 input: {0}")]
    InvalidGitSha1Input(String),
    #[error("invalid path '{0}': {1}")]
    InvalidPath(String, String),
    #[error("invalid Mononoke path '{0}': {1}")]
    InvalidMPath(MPath, String),
    #[error("error while deserializing blob for '{0}'")]
    BlobDeserializeError(String),
    #[error("invalid Thrift structure '{0}': {1}")]
    InvalidThrift(String, String),
    #[error("invalid changeset date: {0}")]
    InvalidDateTime(String),
    #[error("not path-conflict-free: changed path '{0}' is a prefix of '{1}'")]
    NotPathConflictFree(MPath, MPath),
    #[error("invalid bonsai changeset: {0}")]
    InvalidBonsaiChangeset(String),
    #[error("Failed to parse RepositoryId from '{0}'")]
    FailedToParseRepositoryId(String),
}
