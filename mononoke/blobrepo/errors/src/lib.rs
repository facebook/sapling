/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use ascii::AsciiString;
use mercurial_types::{
    blobs::HgBlobChangeset, HgBlob, HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash,
    HgParents, MPath, RepoPath, Type,
};
use mononoke_types::{hash::Sha256, ChangesetId, FileType};
use std::fmt;
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Error while opening state for {0}")]
    StateOpen(StateOpenError),
    #[error("Changeset id {0} is missing")]
    ChangesetMissing(HgChangesetId),
    #[error("Error while deserializing changeset retrieved from key '{0}'")]
    ChangesetDeserializeFailed(String),
    #[error("Manifest id {0} is missing")]
    ManifestMissing(HgManifestId),
    #[error("Node id {0} is missing")]
    NodeMissing(HgNodeHash),
    #[error("Content missing nodeid {0}")]
    ContentMissing(HgNodeHash),
    #[error("Error while deserializing file contents retrieved from key '{0}'")]
    FileContentsDeserializeFailed(String),
    #[error("Content blob missing for id: {0}")]
    ContentBlobByAliasMissing(Sha256),
    #[error("Uploaded blob is incomplete {0:?}")]
    BadUploadBlob(HgBlob),
    #[error("HgParents are not in blob store {0:?}")]
    ParentsUnknown(HgParents),
    #[error("Serialization of node failed {0} ({1})")]
    SerializationFailed(HgNodeHash, bincode::Error),
    #[error("Root manifest is not a manifest (type {0})")]
    BadRootManifest(Type),
    #[error("Manifest type {0} does not match uploaded type {1}")]
    ManifestTypeMismatch(Type, Type),
    #[error("Node generation failed for unknown reason")]
    NodeGenerationFailed,
    #[error("Path {0} appears multiple times in manifests")]
    DuplicateEntry(RepoPath),
    #[error("Duplicate manifest hash {0}")]
    DuplicateManifest(HgNodeHash),
    #[error("Missing entries in new changeset {0}")]
    MissingEntries(HgNodeHash),
    #[error("Filenode is missing: {0} {1}")]
    MissingFilenode(RepoPath, HgFileNodeId),
    #[error("Some manifests do not exist")]
    MissingManifests,
    #[error("Expected {0} to be a manifest, found a {1} instead")]
    NotAManifest(HgNodeHash, Type),
    #[error(
        "Inconsistent node hash for changeset: provided: {0}, \
         computed: {1} for blob: {2:#?}"
    )]
    InconsistentChangesetHash(HgNodeHash, HgNodeHash, HgBlobChangeset),
    #[error("Bookmark {0} does not exist")]
    BookmarkNotFound(AsciiString),
    #[error("Unresolved conflict at {0} with parents: {1:?}")]
    UnresolvedConflicts(MPath, Vec<(FileType, HgFileNodeId)>),
    #[error("Manifest without parents did not get changed by a BonsaiChangeset")]
    UnchangedManifest,
    #[error("Saving empty manifest which is not a root: {0}")]
    SavingHgEmptyManifest(RepoPath),
    #[error("Trying to merge a manifest with two existing parents p1 {0} and p2 {1}")]
    ManifestAlreadyAMerge(HgNodeHash, HgNodeHash),
    #[error("Path not found: {0}")]
    PathNotFound(MPath),
    #[error("Remove called on non-directory")]
    NotADirectory,
    #[error("Empty file path")]
    EmptyFilePath,
    #[error("Memory manifest conflict can not contain single entry")]
    SingleEntryConflict,
    #[error("Cannot find cache pool {0}")]
    MissingCachePool(String),
    #[error("Bonsai cs {0} not found")]
    BonsaiNotFound(ChangesetId),
    #[error("Bonsai changeset not found for hg changeset {0}")]
    BonsaiMappingNotFound(HgChangesetId),
    #[error("Root path wasn't expected at this context")]
    UnexpectedRootPath,
    #[error("Incorrect copy info: not found a file version {from_path} {from_node} the file {to_path} {to_node} was copied from")]
    IncorrectCopyInfo {
        from_path: MPath,
        from_node: HgFileNodeId,
        to_path: MPath,
        to_node: HgFileNodeId,
    },
    #[error("Case conflict in a commit")]
    CaseConflict(MPath),
}
