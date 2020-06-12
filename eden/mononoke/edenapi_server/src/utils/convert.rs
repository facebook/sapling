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

use anyhow::Result;
use mononoke_api::path::MononokePath;
use mononoke_types::MPath;
use types::{RepoPath, RepoPathBuf};

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into a `MononokePath`.
/// The input will be copied due to differences in data representation.
pub fn to_mononoke_path(path: impl AsRef<RepoPath>) -> Result<MononokePath> {
    Ok(MononokePath::new(to_mpath(path)?))
}

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into an `Option<MPath>`.
/// The input will be copied due to differences in data representation.
pub fn to_mpath(path: impl AsRef<RepoPath>) -> Result<Option<MPath>> {
    MPath::new_opt(path.as_ref().as_byte_slice())
}

/// Convert a `MononokePath` into a Mercurial `RepoPathBuf`.
/// The input will be copied due to differences in data representation.
pub fn to_hg_path(path: &MononokePath) -> Result<RepoPathBuf> {
    Ok(match path.as_mpath() {
        Some(mpath) => RepoPathBuf::from_utf8(mpath.to_vec())?,
        None => RepoPathBuf::new(),
    })
}
