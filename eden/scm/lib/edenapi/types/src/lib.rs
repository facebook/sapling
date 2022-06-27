/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Types shared between the EdenAPI client and server.
//!
//! This crate exists primarily to provide a lightweight place to
//! put types that need to be used by both the client and server.
//! Types that are exclusive used by either the client or server
//! SHOULD NOT be added to this crate.
//!
//! Given that the client and server are each part of different
//! projects (Mercurial and Mononoke, respectively) which have
//! different build processes, putting shared types in their own
//! crate decreases the likelihood of build failures caused by
//! dependencies with complex or esoteric build requirements.
//!
//! Most of the types in this crate are used for data interchange
//! between the client and server. As such, CHANGES TO THE THESE
//! TYPES MAY CAUSE VERSION SKEW, so any changes should proceed
//! with caution.

#[macro_use]
pub mod hash;

pub mod anyid;
pub mod batch;
pub mod bookmark;
pub mod commit;
pub mod commitid;
pub mod errors;
pub mod file;
pub mod history;
pub mod land;
pub mod metadata;
pub mod token;
pub mod tree;
pub mod wire;

use bytes::Bytes;
// re-export CloneData
pub use dag_types::CloneData;
pub use dag_types::FlatSegment;
pub use dag_types::Location as CommitLocation;
pub use dag_types::PreparedFlatSegments;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use thiserror::Error;
pub use types::hgid::HgId;
pub use types::key::Key;
pub use types::nodeinfo::NodeInfo;
pub use types::parents::Parents;
pub use types::path::RepoPathBuf;

pub use crate::anyid::AnyId;
pub use crate::anyid::LookupRequest;
pub use crate::anyid::LookupResponse;
pub use crate::anyid::LookupResult;
pub use crate::batch::Batch;
pub use crate::bookmark::BookmarkEntry;
pub use crate::bookmark::BookmarkRequest;
pub use crate::bookmark::SetBookmarkRequest;
pub use crate::commit::make_hash_lookup_request;
pub use crate::commit::BonsaiChangesetContent;
pub use crate::commit::BonsaiFileChange;
pub use crate::commit::CommitGraphEntry;
pub use crate::commit::CommitGraphRequest;
pub use crate::commit::CommitHashLookupRequest;
pub use crate::commit::CommitHashLookupResponse;
pub use crate::commit::CommitHashToLocationRequestBatch;
pub use crate::commit::CommitHashToLocationResponse;
pub use crate::commit::CommitKnownResponse;
pub use crate::commit::CommitLocationToHashRequest;
pub use crate::commit::CommitLocationToHashRequestBatch;
pub use crate::commit::CommitLocationToHashResponse;
pub use crate::commit::CommitMutationsRequest;
pub use crate::commit::CommitMutationsResponse;
pub use crate::commit::CommitRevlogData;
pub use crate::commit::CommitRevlogDataRequest;
pub use crate::commit::CommitTranslateIdRequest;
pub use crate::commit::CommitTranslateIdResponse;
pub use crate::commit::EphemeralPrepareRequest;
pub use crate::commit::EphemeralPrepareResponse;
pub use crate::commit::Extra;
pub use crate::commit::FetchSnapshotRequest;
pub use crate::commit::FetchSnapshotResponse;
pub use crate::commit::HgChangesetContent;
pub use crate::commit::HgMutationEntryContent;
pub use crate::commit::SnapshotRawData;
pub use crate::commit::SnapshotRawFiles;
pub use crate::commit::UploadBonsaiChangesetRequest;
pub use crate::commit::UploadHgChangeset;
pub use crate::commit::UploadHgChangesetsRequest;
pub use crate::commit::UploadSnapshotResponse;
pub use crate::commitid::BonsaiChangesetId;
pub use crate::commitid::CommitId;
pub use crate::commitid::CommitIdScheme;
pub use crate::commitid::GitSha1;
pub use crate::errors::ServerError;
pub use crate::file::FileAttributes;
pub use crate::file::FileAuxData;
pub use crate::file::FileContent;
pub use crate::file::FileEntry;
pub use crate::file::FileError;
pub use crate::file::FileRequest;
pub use crate::file::FileResponse;
pub use crate::file::FileSpec;
pub use crate::file::HgFilenodeData;
pub use crate::file::UploadHgFilenodeRequest;
pub use crate::file::UploadTokensResponse;
pub use crate::history::HistoryEntry;
pub use crate::history::HistoryRequest;
pub use crate::history::HistoryResponse;
pub use crate::history::HistoryResponseChunk;
pub use crate::history::WireHistoryEntry;
pub use crate::land::LandStackRequest;
pub use crate::land::LandStackResponse;
pub use crate::land::PushVar;
pub use crate::metadata::AnyFileContentId;
pub use crate::metadata::ContentId;
pub use crate::metadata::DirectoryMetadata;
pub use crate::metadata::DirectoryMetadataRequest;
pub use crate::metadata::FileMetadata;
pub use crate::metadata::FileMetadataRequest;
pub use crate::metadata::FileType;
pub use crate::metadata::FsnodeId;
pub use crate::metadata::Sha1;
pub use crate::metadata::Sha256;
pub use crate::token::FileContentTokenMetadata;
pub use crate::token::IndexableId;
pub use crate::token::UploadToken;
pub use crate::token::UploadTokenData;
pub use crate::token::UploadTokenMetadata;
pub use crate::token::UploadTokenSignature;
pub use crate::tree::TreeAttributes;
pub use crate::tree::TreeChildDirectoryEntry;
pub use crate::tree::TreeChildEntry;
pub use crate::tree::TreeChildFileEntry;
pub use crate::tree::TreeEntry;
pub use crate::tree::TreeError;
pub use crate::tree::TreeRequest;
pub use crate::tree::UploadTreeEntry;
pub use crate::tree::UploadTreeRequest;
pub use crate::tree::UploadTreeResponse;
pub use crate::wire::ToApi;
pub use crate::wire::ToWire;
pub use crate::wire::WireToApiConversionError;

#[derive(Debug, Error)]
#[error("Invalid hash: {expected} (expected) != {computed} (computed)")]
pub struct InvalidHgId {
    expected: HgId,
    computed: HgId,
    data: Bytes,
    parents: Parents,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Error fetching key {key:?}: {err}")]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct EdenApiServerError {
    pub err: EdenApiServerErrorKind,
    pub key: Option<Key>,
}

impl EdenApiServerError {
    pub fn new(err: impl std::fmt::Debug) -> EdenApiServerError {
        EdenApiServerError {
            err: EdenApiServerErrorKind::OpaqueError(format!("{:?}", err)),
            key: None,
        }
    }

    pub fn with_key(key: Key, err: impl std::fmt::Debug) -> EdenApiServerError {
        EdenApiServerError {
            err: EdenApiServerErrorKind::OpaqueError(format!("{:?}", err)),
            key: Some(key),
        }
    }

    pub fn with_path(path: RepoPathBuf, err: impl std::fmt::Debug) -> EdenApiServerError {
        EdenApiServerError {
            err: EdenApiServerErrorKind::OpaqueError(format!("{:?}", err)),
            key: Some(Key {
                path,
                hgid: *HgId::null_id(),
            }),
        }
    }

    pub fn with_hgid(hgid: HgId, err: impl std::fmt::Debug) -> EdenApiServerError {
        EdenApiServerError {
            err: EdenApiServerErrorKind::OpaqueError(format!("{:?}", err)),
            key: Some(Key {
                hgid,
                path: RepoPathBuf::new(),
            }),
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum EdenApiServerErrorKind {
    #[error("EdenAPI server returned an error with message: {0}")]
    OpaqueError(String),
}
