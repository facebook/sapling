// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use ascii::AsciiString;
use bincode;

pub use failure::{Error, ResultExt};

use mercurial::HgNodeHash;
use mercurial_types::{DChangesetId, DFileNodeId, DNodeHash, DParents, HgBlob, HgBlobHash, MPath,
                      RepoPath, Type};

use BlobChangeset;

#[derive(Debug)]
pub enum StateOpenError {
    Heads,
    Bookmarks,
    Blobstore,
    Changesets,
    Filenodes,
}

impl fmt::Display for StateOpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use StateOpenError::*;

        match *self {
            Heads => write!(f, "heads"),
            Bookmarks => write!(f, "bookmarks"),
            Blobstore => write!(f, "blob store"),
            Changesets => write!(f, "changesets"),
            Filenodes => write!(f, "filenodes"),
        }
    }
}

pub type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Error while opening state for {}", _0)] StateOpen(StateOpenError),
    #[fail(display = "Changeset id {} is missing", _0)] ChangesetMissing(DChangesetId),
    #[fail(display = "Manifest id {} is missing", _0)] ManifestMissing(DNodeHash),
    #[fail(display = "Node id {} is missing", _0)] NodeMissing(DNodeHash),
    #[fail(display = "Content missing nodeid {} (blob hash {:?})", _0, _1)]
    ContentMissing(DNodeHash, HgBlobHash),
    #[fail(display = "Uploaded blob is incomplete {:?}", _0)] BadUploadBlob(HgBlob),
    #[fail(display = "DParents are not in blob store {:?}", _0)] ParentsUnknown(DParents),
    #[fail(display = "Serialization of node failed {} ({})", _0, _1)]
    SerializationFailed(HgNodeHash, bincode::Error),
    #[fail(display = "Root manifest is not a manifest (type {})", _0)] BadRootManifest(Type),
    #[fail(display = "Manifest type {} does not match uploaded type {}", _0, _1)]
    ManifestTypeMismatch(Type, Type),
    #[fail(display = "Node generation failed for unknown reason")] NodeGenerationFailed,
    #[fail(display = "Path {} appears multiple times in manifests", _0)] DuplicateEntry(RepoPath),
    #[fail(display = "Duplicate manifest hash {}", _0)] DuplicateManifest(DNodeHash),
    #[fail(display = "Missing entries in new changeset {}", _0)] MissingEntries(DNodeHash),
    #[fail(display = "Filenode is missing: {} {}", _0, _1)] MissingFilenode(RepoPath, DFileNodeId),
    #[fail(display = "Some manifests do not exist")] MissingManifests,
    #[fail(display = "DParents failed to complete")] ParentsFailed,
    #[fail(display = "Expected {} to be a manifest, found a {} instead", _0, _1)]
    NotAManifest(DNodeHash, Type),
    #[fail(display = "Inconsistent node hash for entry: path {}, provided: {}, computed: {}", _0,
           _1, _2)]
    InconsistentEntryHash(RepoPath, HgNodeHash, HgNodeHash),
    #[fail(display = "Inconsistent node hash for changeset: provided: {}, \
                      computed: {} for blob: {:#?}",
           _0, _1, _2)]
    InconsistentChangesetHash(HgNodeHash, HgNodeHash, BlobChangeset),
    #[fail(display = "Bookmark {} does not exist", _0)] BookmarkNotFound(AsciiString),
    #[fail(display = "Unresolved conflicts when converting BonsaiChangeset to Manifest")]
    UnresolvedConflicts,
    #[fail(display = "Manifest without parents did not get changed by a BonsaiChangeset")]
    UnchangedManifest,
    #[fail(display = "Trying to merge a manifest with two existing parents p1 {} and p2 {}", _0,
           _1)]
    ManifestAlreadyAMerge(HgNodeHash, HgNodeHash),
    #[fail(display = "Path not found: {}", _0)] PathNotFound(MPath),
    #[fail(display = "Remove called on non-directory")] NotADirectory,
}
