/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! These types have a representation that is visible to C / C++, and are either
//! allocated in C / C++ and must be freed accordingly, or do not need to be freed
//! at all (no heap allocations).

use std::convert::TryFrom;

use libc::size_t;

use anyhow::{Error, Result};
use edenapi_types::{
    metadata::{ContentId as ApiContentId, Sha1 as ApiSha1, Sha256 as ApiSha256},
    FileType as ApiFileType, TreeAttributes as ApiTreeAttributes,
};
use types::{HgId as ApiHgId, Key as ApiKey, Parents as ApiParents, RepoPathBuf};

use crate::ptr_len_to_slice;

#[repr(C)]
pub struct Key {
    path: *const u8,
    path_len: size_t,
    hgid: [u8; 20],
}

// Conversion takes a reference to Key, leaving the caller
// responsible for deallocation of the string.
impl TryFrom<&Key> for ApiKey {
    type Error = Error;
    fn try_from(v: &Key) -> Result<Self> {
        let path = unsafe { ptr_len_to_slice(v.path, v.path_len)? };
        // to_vec copies the slice, so the ApiKey can be
        // deallocated in Rust-land.
        let path = RepoPathBuf::from_utf8(path.to_vec())?;
        Ok(Self {
            path,
            hgid: ApiHgId::from_byte_array(v.hgid),
        })
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub enum FileType {
    Regular,
    Executable,
    Symlink,
}

impl From<ApiFileType> for FileType {
    fn from(v: ApiFileType) -> Self {
        match v {
            ApiFileType::Regular => FileType::Regular,
            ApiFileType::Executable => FileType::Executable,
            ApiFileType::Symlink => FileType::Symlink,
        }
    }
}

#[repr(C, u8)]
#[derive(Copy, Clone)]
pub enum Parents {
    None,
    One([u8; 20]),
    Two([u8; 20], [u8; 20]),
}

impl From<ApiParents> for Parents {
    fn from(v: ApiParents) -> Self {
        match v {
            ApiParents::None => Parents::None,
            ApiParents::One(id1) => Parents::One(id1.into_byte_array()),
            ApiParents::Two(id1, id2) => Parents::Two(id1.into_byte_array(), id2.into_byte_array()),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct HgId(pub [u8; 20]);

impl From<ApiHgId> for HgId {
    fn from(v: ApiHgId) -> Self {
        HgId(v.into_byte_array())
    }
}

impl From<[u8; 20]> for HgId {
    fn from(v: [u8; 20]) -> Self {
        HgId(v)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ContentId(pub [u8; 32]);

impl From<ApiContentId> for ContentId {
    fn from(v: ApiContentId) -> Self {
        ContentId(v.0)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Sha1(pub [u8; 20]);

impl From<ApiSha1> for Sha1 {
    fn from(v: ApiSha1) -> Self {
        Sha1(v.0)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Sha256(pub [u8; 32]);

impl From<ApiSha256> for Sha256 {
    fn from(v: ApiSha256) -> Self {
        Sha256(v.0)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TreeAttributes {
    manifest_blob: bool,
    parents: bool,
    child_metadata: bool,
}

impl From<TreeAttributes> for ApiTreeAttributes {
    fn from(v: TreeAttributes) -> Self {
        ApiTreeAttributes {
            manifest_blob: v.manifest_blob,
            parents: v.parents,
            child_metadata: v.child_metadata,
        }
    }
}
