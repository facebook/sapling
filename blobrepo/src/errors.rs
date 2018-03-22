// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use bincode;

pub use failure::{Error, ResultExt};

use mercurial_types::{Blob, HgBlobHash, HgChangesetId, NodeHash, Parents, RepoPath, Type};

#[derive(Debug)]
pub enum StateOpenError {
    Heads,
    Bookmarks,
    Blobstore,
    Changesets,
    Linknodes,
}

impl fmt::Display for StateOpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use StateOpenError::*;

        match *self {
            Heads => write!(f, "heads"),
            Bookmarks => write!(f, "bookmarks"),
            Blobstore => write!(f, "blob store"),
            Changesets => write!(f, "changesets"),
            Linknodes => write!(f, "linknodes"),
        }
    }
}

pub type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Error while opening state for {}", _0)] StateOpen(StateOpenError),
    #[fail(display = "Changeset id {} is missing", _0)] ChangesetMissing(HgChangesetId),
    #[fail(display = "Manifest id {} is missing", _0)] ManifestMissing(NodeHash),
    #[fail(display = "Node id {} is missing", _0)] NodeMissing(NodeHash),
    #[fail(display = "Content missing nodeid {} (blob hash {:?})", _0, _1)]
    ContentMissing(NodeHash, HgBlobHash),
    #[fail(display = "Uploaded blob is incomplete {:?}", _0)] BadUploadBlob(Blob),
    #[fail(display = "Parents are not in blob store {:?}", _0)] ParentsUnknown(Parents),
    #[fail(display = "Serialization of node failed {} ({})", _0, _1)]
    SerializationFailed(NodeHash, bincode::Error),
    #[fail(display = "Root manifest is not a manifest (type {})", _0)] BadRootManifest(Type),
    #[fail(display = "Manifest type {} does not match uploaded type {}", _0, _1)]
    ManifestTypeMismatch(Type, Type),
    #[fail(display = "Node generation failed for unknown reason")] NodeGenerationFailed,
    #[fail(display = "Path {} appears multiple times in manifests", _0)] DuplicateEntry(RepoPath),
    #[fail(display = "Duplicate manifest hash {}", _0)] DuplicateManifest(NodeHash),
    #[fail(display = "Missing entries in new changeset {}", _0)] MissingEntries(NodeHash),
    #[fail(display = "Some manifests do not exist")] MissingManifests,
    #[fail(display = "Parents failed to complete")] ParentsFailed,
    #[fail(display = "Expected {} to be a manifest, found a {} instead", _0, _1)]
    NotAManifest(NodeHash, Type),
}
