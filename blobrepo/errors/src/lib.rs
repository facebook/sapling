/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use ascii::AsciiString;
use failure_ext::failure;
use failure_ext::Fail;
use mercurial_types::{
    blobs::HgBlobChangeset, HgBlob, HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash,
    HgParents, MPath, RepoPath, Type,
};
use mononoke_types::{hash::Sha256, ChangesetId};
use std::fmt;

#[derive(Debug)]
pub enum StateOpenError {
    Heads,
    Bookmarks,
    Changesets,
    Filenodes,
    BonsaiGlobalrevMapping,
    BonsaiHgMapping,
}

impl fmt::Display for StateOpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            StateOpenError::Heads => write!(f, "heads"),
            StateOpenError::Bookmarks => write!(f, "bookmarks"),
            StateOpenError::Changesets => write!(f, "changesets"),
            StateOpenError::Filenodes => write!(f, "filenodes"),
            StateOpenError::BonsaiGlobalrevMapping => write!(f, "bonsai_globalrev_mapping"),
            StateOpenError::BonsaiHgMapping => write!(f, "bonsai_hg_mapping"),
        }
    }
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Missing typed key entry for key: {}", _0)]
    MissingTypedKeyEntry(String),
    // TODO(anastasiyaz): Use general Alias Key instead of Sha256
    #[fail(display = "Incorrect content of alias blob: {}", _0)]
    IncorrectAliasBlobContent(Sha256),
    #[fail(display = "Error while opening state for {}", _0)]
    StateOpen(StateOpenError),
    #[fail(display = "Changeset id {} is missing", _0)]
    ChangesetMissing(HgChangesetId),
    #[fail(
        display = "Error while deserializing changeset retrieved from key '{}'",
        _0
    )]
    ChangesetDeserializeFailed(String),
    #[fail(display = "Manifest id {} is missing", _0)]
    ManifestMissing(HgManifestId),
    #[fail(display = "Node id {} is missing", _0)]
    NodeMissing(HgNodeHash),
    #[fail(display = "Content missing nodeid {}", _0)]
    ContentMissing(HgNodeHash),
    #[fail(
        display = "Error while deserializing file contents retrieved from key '{}'",
        _0
    )]
    FileContentsDeserializeFailed(String),
    #[fail(display = "Content blob missing for id: {}", _0)]
    ContentBlobByAliasMissing(Sha256),
    #[fail(display = "Uploaded blob is incomplete {:?}", _0)]
    BadUploadBlob(HgBlob),
    #[fail(display = "HgParents are not in blob store {:?}", _0)]
    ParentsUnknown(HgParents),
    #[fail(display = "Serialization of node failed {} ({})", _0, _1)]
    SerializationFailed(HgNodeHash, bincode::Error),
    #[fail(display = "Root manifest is not a manifest (type {})", _0)]
    BadRootManifest(Type),
    #[fail(display = "Manifest type {} does not match uploaded type {}", _0, _1)]
    ManifestTypeMismatch(Type, Type),
    #[fail(display = "Node generation failed for unknown reason")]
    NodeGenerationFailed,
    #[fail(display = "Path {} appears multiple times in manifests", _0)]
    DuplicateEntry(RepoPath),
    #[fail(display = "Duplicate manifest hash {}", _0)]
    DuplicateManifest(HgNodeHash),
    #[fail(display = "Missing entries in new changeset {}", _0)]
    MissingEntries(HgNodeHash),
    #[fail(display = "Filenode is missing: {} {}", _0, _1)]
    MissingFilenode(RepoPath, HgFileNodeId),
    #[fail(display = "Some manifests do not exist")]
    MissingManifests,
    #[fail(display = "Expected {} to be a manifest, found a {} instead", _0, _1)]
    NotAManifest(HgNodeHash, Type),
    #[fail(
        display = "Inconsistent node hash for changeset: provided: {}, \
                   computed: {} for blob: {:#?}",
        _0, _1, _2
    )]
    InconsistentChangesetHash(HgNodeHash, HgNodeHash, HgBlobChangeset),
    #[fail(display = "Bookmark {} does not exist", _0)]
    BookmarkNotFound(AsciiString),
    #[fail(display = "Unresolved conflicts when converting BonsaiChangeset to Manifest")]
    UnresolvedConflicts,
    #[fail(display = "Manifest without parents did not get changed by a BonsaiChangeset")]
    UnchangedManifest,
    #[fail(display = "Saving empty manifest which is not a root: {}", _0)]
    SavingHgEmptyManifest(RepoPath),
    #[fail(
        display = "Trying to merge a manifest with two existing parents p1 {} and p2 {}",
        _0, _1
    )]
    ManifestAlreadyAMerge(HgNodeHash, HgNodeHash),
    #[fail(display = "Path not found: {}", _0)]
    PathNotFound(MPath),
    #[fail(display = "Remove called on non-directory")]
    NotADirectory,
    #[fail(display = "Empty file path")]
    EmptyFilePath,
    #[fail(display = "Memory manifest conflict can not contain single entry")]
    SingleEntryConflict,
    #[fail(display = "Cannot find cache pool {}", _0)]
    MissingCachePool(String),
    #[fail(display = "Bonsai cs {} not found", _0)]
    BonsaiNotFound(ChangesetId),
    #[fail(display = "Bonsai changeset not found for hg changeset {}", _0)]
    BonsaiMappingNotFound(HgChangesetId),
    #[fail(display = "Root path wasn't expected at this context")]
    UnexpectedRootPath,
    #[fail(
        display = "Incorrect copy info: not found a file version {} {} the file {} {} was copied from",
        from_path, from_node, to_path, to_node
    )]
    IncorrectCopyInfo {
        from_path: MPath,
        from_node: HgFileNodeId,
        to_path: MPath,
        to_node: HgFileNodeId,
    },
    #[fail(display = "Case conflict in a commit")]
    CaseConflict(MPath),
    #[fail(display = "Mercurial entry can not have more than two parents")]
    TooManyParents,
}
