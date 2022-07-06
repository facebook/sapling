/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Base types used throughout Mononoke.

pub mod blame;
pub mod blame_v2;
pub mod blob;
pub mod bonsai_changeset;
pub mod content_chunk;
pub mod content_metadata;
pub mod content_metadata_v2;
pub mod datetime;
pub mod deleted_manifest_common;
pub mod deleted_manifest_v2;
pub mod errors;
pub mod fastlog_batch;
pub mod file_change;
pub mod file_contents;
pub mod fsnode;
pub mod generation;
pub mod globalrev;
pub mod hash;
pub mod path;
pub mod rawbundle2;
pub mod redaction_key_list;
pub mod repo;
pub mod sharded_map;
pub mod skeleton_manifest;
pub mod sql_types;
pub mod svnrev;
pub mod thrift_convert;
pub mod typed_hash;
pub mod unode;

pub use blame::Blame;
pub use blame::BlameId;
pub use blame::BlameRange;
pub use blob::Blob;
pub use blob::BlobstoreValue;
pub use blob::ChangesetBlob;
pub use blob::ContentBlob;
pub use blob::RawBundle2Blob;
pub use blobstore::BlobstoreBytes;
pub use bonsai_changeset::BonsaiChangeset;
pub use bonsai_changeset::BonsaiChangesetMut;
pub use content_chunk::ContentChunk;
pub use content_metadata::ContentAlias;
pub use content_metadata::ContentMetadata;
pub use datetime::DateTime;
pub use datetime::Timestamp;
pub use file_change::BasicFileChange;
pub use file_change::FileChange;
pub use file_change::FileType;
pub use file_change::TrackedFileChange;
pub use file_contents::ChunkedFileContents;
pub use file_contents::ContentChunkPointer;
pub use file_contents::FileContents;
pub use generation::Generation;
pub use generation::FIRST_GENERATION;
pub use globalrev::Globalrev;
pub use path::check_case_conflicts;
pub use path::mpath_element_iter;
pub use path::path_bytes_from_mpath;
pub use path::MPath;
pub use path::MPathElement;
pub use path::MPathHash;
pub use path::PrefixTrie;
pub use path::RepoPath;
pub use rawbundle2::RawBundle2;
pub use redaction_key_list::RedactionKeyList;
pub use repo::RepositoryId;
pub use repo::REPO_PREFIX_REGEX;
pub use svnrev::Svnrev;
pub use thrift_convert::ThriftConvert;
pub use typed_hash::BlobstoreKey;
pub use typed_hash::ChangesetId;
pub use typed_hash::ChangesetIdPrefix;
pub use typed_hash::ChangesetIdsResolvedFromPrefix;
pub use typed_hash::ContentChunkId;
pub use typed_hash::ContentId;
pub use typed_hash::ContentMetadataId;
pub use typed_hash::DeletedManifestV2Id;
pub use typed_hash::FastlogBatchId;
pub use typed_hash::FileUnodeId;
pub use typed_hash::FsnodeId;
pub use typed_hash::ManifestUnodeId;
pub use typed_hash::MononokeId;
pub use typed_hash::RawBundle2Id;
pub use typed_hash::SkeletonManifestId;

mod macros;

pub mod thrift {
    pub use mononoke_types_thrift::*;
}

pub mod private {
    pub use anyhow;
    pub use ascii::AsciiStr;
    pub use ascii::AsciiString;
    pub use bytes::Bytes;
    pub use quickcheck::empty_shrinker;
    pub use quickcheck::Arbitrary;
    pub use quickcheck::Gen;
    pub use serde::de::Deserialize;
    pub use serde::de::Deserializer;
    pub use serde::de::Error as DeError;
    pub use serde::Serialize;
    pub use serde::Serializer;

    pub use crate::errors::ErrorKind;
    pub use crate::hash::Blake2;
    pub use crate::thrift;
    pub use crate::typed_hash::Blake2HexVisitor;
}
