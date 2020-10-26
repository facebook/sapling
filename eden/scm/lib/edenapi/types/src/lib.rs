/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

#![deny(warnings)]

pub mod commit;
pub mod complete_tree;
pub mod file;
pub mod history;
pub mod json;
pub mod metadata;
pub mod tree;
pub mod wire;

pub use crate::commit::{
    CommitLocation, CommitLocationToHash, CommitLocationToHashRequest, CommitRevlogData,
    CommitRevlogDataRequest,
};
pub use crate::complete_tree::CompleteTreeRequest;
pub use crate::file::{FileEntry, FileError, FileRequest};
pub use crate::history::{
    HistoryEntry, HistoryRequest, HistoryResponse, HistoryResponseChunk, WireHistoryEntry,
};
pub use crate::metadata::{
    ContentId, DirectoryMetadata, DirectoryMetadataRequest, FileMetadata, FileMetadataRequest,
    FileType, FsnodeId, Sha1, Sha256,
};
pub use crate::tree::{TreeChildEntry, TreeEntry, TreeError, TreeRequest};
pub use crate::wire::{ToApi, ToWire, WireToApiConversionError};

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use thiserror::Error;

use bytes::Bytes;
use types::{hgid::HgId, key::Key, parents::Parents, path::RepoPathBuf};

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
pub enum EdenApiServerErrorKind {
    #[error("EdenAPI server returned an error with message: {0}")]
    OpaqueError(String),
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for EdenApiServerError {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            err: Arbitrary::arbitrary(g),
            key: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for EdenApiServerErrorKind {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        use rand::Rng;
        let variant = g.gen_range(0, 1);
        match variant {
            0 => EdenApiServerErrorKind::OpaqueError(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}
