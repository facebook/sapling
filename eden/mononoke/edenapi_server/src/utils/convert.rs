/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! convert.rs - Conversions between Mercurial and Mononoke types.
//!
//! Mercurial and Mononoke use different types to represent similar
//! concepts, such as paths, identifiers, etc. While these types
//! fundamentally represent the same things, they often differ in
//! implementation details, adding some friction when converting.
//!
//! In theory, the conversions should never fail since these types
//! are used to represent the same data on the client and server
//! respectively, so any conversion failure should be considered
//! a bug. Nonetheless, since these types often differ substantially
//! in implentation, it is possible that conversion failures may occur
//! in practice.

use anyhow::{Context, Result};
use mononoke_api::path::MononokePath;
use mononoke_types::MPath;
use types::{RepoPath, RepoPathBuf};

use crate::errors::ErrorKind;

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into a `MononokePath`.
/// The input will be copied due to differences in data representation.
pub fn to_mononoke_path(path: impl AsRef<RepoPath>) -> Result<MononokePath> {
    Ok(MononokePath::new(to_mpath(path)?))
}

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into an `Option<MPath>`.
/// The input will be copied due to differences in data representation.
pub fn to_mpath(path: impl AsRef<RepoPath>) -> Result<Option<MPath>> {
    let path_bytes = path.as_ref().as_byte_slice();
    MPath::new_opt(path_bytes).with_context(|| ErrorKind::InvalidPath(path_bytes.to_vec()))
}

/// Convert a `MononokePath` into a Mercurial `RepoPathBuf`.
/// The input will be copied due to differences in data representation.
pub fn to_hg_path(path: &MononokePath) -> Result<RepoPathBuf> {
    let path_bytes = match path.as_mpath() {
        Some(mpath) => mpath.to_vec(),
        None => return Ok(RepoPathBuf::new()),
    };
    RepoPathBuf::from_utf8(path_bytes.clone()).context(ErrorKind::InvalidPath(path_bytes))
}
