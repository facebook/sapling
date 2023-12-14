/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use filestore::FetchKey;
use megarepo_error::cloneable_error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MononokeGitError {
    #[error("Could not locate content: {0:?}")]
    ContentMissing(FetchKey),
    #[error("Tree Derivation Failed")]
    TreeDerivationFailed,
    #[error("Invalid Thrift")]
    InvalidThrift,
}

#[derive(Clone, Debug, Error)]
pub enum GitError {
    /// The provided hash and the derived hash do not match for the given content.
    #[error("Input hash {0} does not match the SHA1 hash {1} of the content")]
    HashMismatch(String, String),

    /// The input hash is not a valid SHA1 hash.
    #[error("Input hash {0} is not a valid SHA1 git hash")]
    InvalidHash(String),

    /// The packfile item stored for the input Git hash is invalid.
    #[error("Invalid packfile item stored for git object ID {0}")]
    InvalidPackfileItem(String),

    /// The raw object content provided do not correspond to a valid git object.
    #[error("Invalid git object content provided for object ID {0}. Cause: {1}")]
    InvalidContent(String, GitInternalError),

    /// The requested bubble does not exist.  Either it was never created or has expired.
    #[error(
        "The object corresponding to object ID {0} is a git blob. Cannot upload raw blob content"
    )]
    DisallowedBlobObject(String),

    /// Failed to get or store the git object in Mononoke store.
    #[error(
        "Failed to get or store the git object (ID: {0}) or its packfile item in blobstore. Cause: {1}"
    )]
    StorageFailure(String, GitInternalError),

    /// The git object doesn't exist in the Mononoke store.
    #[error(
        "The object corresponding to object ID {0} or its packfile item does not exist in the data store"
    )]
    NonExistentObject(String),

    /// The provided git object could not be converted to a valid bonsai changeset.
    #[error(
        "Validation failure while persisting git object (ID: {0}) as a bonsai changeset. Cause: {1}"
    )]
    InvalidBonsai(String, GitInternalError),

    /// Error during the packfile stream generation or while writing it to bundle or while storing
    /// to everstore
    #[error("{0}")]
    PackfileError(String),
}

cloneable_error!(GitInternalError);
