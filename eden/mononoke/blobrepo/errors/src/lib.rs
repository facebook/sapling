/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ascii::AsciiString;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgBlob;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::Type;
use mononoke_types::hash::Sha256;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use std::fmt;
use thiserror::Error;

#[derive(Debug)]
pub enum StateOpenError {
    Heads,
    Bookmarks,
    Changesets,
    Filenodes,
    BonsaiGitMapping,
    BonsaiGlobalrevMapping,
    BonsaiSvnrevMapping,
    BonsaiHgMapping,
    Phases,
    HgMutationStore,
    SegmentedChangelog,
}

impl fmt::Display for StateOpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            StateOpenError::Heads => write!(f, "heads"),
            StateOpenError::Bookmarks => write!(f, "bookmarks"),
            StateOpenError::Changesets => write!(f, "changesets"),
            StateOpenError::Filenodes => write!(f, "filenodes"),
            StateOpenError::BonsaiGitMapping => write!(f, "bonsai_git_mapping"),
            StateOpenError::BonsaiGlobalrevMapping => write!(f, "bonsai_globalrev_mapping"),
            StateOpenError::BonsaiSvnrevMapping => write!(f, "bonsai_svnrev_mapping"),
            StateOpenError::BonsaiHgMapping => write!(f, "bonsai_hg_mapping"),
            StateOpenError::Phases => write!(f, "phases"),
            StateOpenError::HgMutationStore => write!(f, "hg_mutation_store"),
            StateOpenError::SegmentedChangelog => write!(f, "segmented_changelog"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Error while opening state for {0}")]
    StateOpen(StateOpenError),
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
    BadRootManifest(HgManifestId),
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
    #[error(
        "Incorrect copy info: not found a file version {from_path} {from_node} the file {to_path} {to_node} was copied from"
    )]
    IncorrectCopyInfo {
        from_path: MPath,
        from_node: HgFileNodeId,
        to_path: MPath,
        to_node: HgFileNodeId,
    },
    #[error(
        "CaseConflict: the changes introduced by this commit have conflicting case. The first offending path is '{0}', and conflicted with '{1}'. Resolve the conflict."
    )]
    InternalCaseConflict(MPath, MPath),
    #[error(
        "CaseConflict: the changes introduced by this commit conflict with existing files in the repository. The first conflicting path in this commit was '{0}', and conflicted with '{1}' in the repository. Resolve the conflict."
    )]
    ExternalCaseConflict(MPath, MPath),
}
