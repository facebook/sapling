// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Base types used throughout Mononoke.

#![deny(warnings)]
#![feature(const_fn)]

pub mod blob;
pub mod bonsai_changeset;
pub mod content_chunk;
pub mod content_metadata;
pub mod datetime;
pub mod errors;
pub mod file_change;
pub mod file_contents;
pub mod generation;
pub mod hash;
pub mod path;
pub mod rawbundle2;
pub mod repo;
pub mod sql_types;
pub mod typed_hash;
pub mod unode;

pub use blob::{Blob, BlobstoreValue, ChangesetBlob, ContentBlob, RawBundle2Blob};
pub use blobstore::BlobstoreBytes;
pub use bonsai_changeset::{BonsaiChangeset, BonsaiChangesetMut};
pub use content_chunk::ContentChunk;
pub use content_metadata::{ContentAlias, ContentMetadata};
pub use datetime::{DateTime, Timestamp};
pub use file_change::{FileChange, FileType};
pub use file_contents::{ChunkedFileContents, ContentChunkPointer, FileContents};
pub use generation::Generation;
pub use path::{check_case_conflicts, MPath, MPathElement, MPathHash, RepoPath, RepoPathCached};
pub use rawbundle2::RawBundle2;
pub use repo::RepositoryId;
pub use typed_hash::{
    ChangesetId, ContentChunkId, ContentId, ContentMetadataId, FileUnodeId, ManifestUnodeId,
    MononokeId, RawBundle2Id,
};

mod macros;

mod thrift {
    pub use mononoke_types_thrift::*;
}
