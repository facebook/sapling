/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! File operations that do not follow symlinks below a root directory.
//!
//! The root path itself is opened normally and may traverse symlinks. All
//! paths passed to [`NoFollowRoot`] methods are converted to [`CheckedRelPath`]
//! before any filesystem operation.

use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
mod tests;
mod types;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub use types::LiteMetadata;
pub use types::OpenFlags;
#[cfg(unix)]
pub use unix::AtomicReplaceFile;
#[cfg(unix)]
pub use unix::NoFollowRoot;
#[cfg(windows)]
pub use windows::AtomicReplaceFile;
#[cfg(windows)]
pub use windows::NoFollowRoot;

/// A verified repository-relative path that cannot escape upward.
///
/// This path is relative, non-empty, and contains no `..` components. It may be
/// constructed by validating a [`Path`], or by another crate from a stronger
/// path type that already enforces the same invariant.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CheckedRelPath<'a>(&'a Path);

impl<'a> CheckedRelPath<'a> {
    /// Construct a path from an already-verified relative path.
    ///
    /// This is intended for stronger path newtypes, such as repository path
    /// types, that already reject absolute paths and `..`.
    pub fn from_verified_relative(path: &'a Path) -> Self {
        Self(path)
    }

    pub(crate) fn as_path(&self) -> &Path {
        self.0
    }
}

impl<'a> TryFrom<&'a Path> for CheckedRelPath<'a> {
    type Error = io::Error;

    fn try_from(path: &'a Path) -> io::Result<Self> {
        let has_normal_component =
            path.components()
                .try_fold(false, |has_normal, component| match component {
                    Component::Normal(_) => Ok(true),
                    Component::CurDir => Ok(has_normal),
                    Component::ParentDir => Err(invalid_path(path, "path contains `..`")),
                    Component::RootDir | Component::Prefix(_) => {
                        Err(invalid_path(path, "path must be relative"))
                    }
                })?;

        if !has_normal_component {
            return Err(invalid_path(path, "path must name a file or directory"));
        }

        Ok(Self(path))
    }
}

impl<'a> TryFrom<&'a PathBuf> for CheckedRelPath<'a> {
    type Error = io::Error;

    fn try_from(path: &'a PathBuf) -> io::Result<Self> {
        path.as_path().try_into()
    }
}

fn invalid_path(path: &Path, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{message}: {:?}", path),
    )
}

#[cfg(any(unix, windows))]
pub(crate) fn normalize_not_directory(err: io::Error) -> io::Error {
    // Used by `symlink_metadata(path)`.
    // If `path` contains a directory that doesn't actually exist on disk, it surfaces as a
    // NotADirectory error. This error type is unstable and can't actually be matched on.
    // See https://github.com/rust-lang/rust/issues/86442
    // For now, let's convert it to a NotFound error, users probably want to treat it as such.
    #[cfg(unix)]
    const NOTDIR: i32 = 20; // ENOTDIR
    #[cfg(windows)]
    const NOTDIR: i32 = 267; // ERROR_DIRECTORY

    match err.raw_os_error() {
        Some(errno) if errno == NOTDIR => io::Error::new(io::ErrorKind::NotFound, err),
        _ => err,
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn normalize_not_directory(err: io::Error) -> io::Error {
    err
}
