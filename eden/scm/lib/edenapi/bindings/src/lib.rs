/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod client;
mod init;
pub mod opaque;
pub mod owned;
pub mod resultutil;
//mod tests;
pub mod types;
pub mod vecutil;

pub use crate::{
    opaque::{ApiKey, EdenApiError, EdenApiServerError, FileMetadata, TreeChildEntry, TreeEntry},
    owned::{EdenApiClient, OwnedString, TreeEntryFetch},
    types::{ContentId, FileType, HgId, Key, Parents, Sha1, Sha256},
};

use std::slice;

use anyhow::Error;
use libc::size_t;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("null ptr")]
pub struct PtrToSliceErr;

impl From<PtrToSliceErr> for EdenApiError {
    fn from(v: PtrToSliceErr) -> Self {
        let anyhow: Error = v.into();
        anyhow.into()
    }
}

/// Convert a pointer-length array to a slice
///
/// This method does not check all required invariants. See
/// slice::from_raw_parts for details.
pub unsafe fn ptr_len_to_slice<'a, T>(
    ptr: *const T,
    len: size_t,
) -> Result<&'a [T], PtrToSliceErr> {
    //ensure!(!ptr.is_null(), "ptr is null");
    if ptr.is_null() {
        Err(PtrToSliceErr)
    } else {
        Ok(slice::from_raw_parts(ptr, len))
    }
}
