// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! manifest - The contents of the repository at a specific commit.
//!
//! The history of the repository is recorded in the commit graph. Each commit has a manifest
//! associated with it. The manifest specifies the revision for all the files available in the
//! repository. The file path and file revision are then used to retrieve the contents of the
//! file thus achieving the reconstruction of the entire repository state.

use failure::Fallible;

use types::{Node, RepoPath, RepoPathBuf};

/// Manifest describes a mapping between file path ([`String`]) and file metadata ([`FileMetadata`]).
/// Fundamentally it is just a Map<file_path, file_metadata>.
///
/// It can be assumed that Manifest interacts with an underlying store for persistance. These
/// interactions may fail due to a variety of reasons. Such failures will be propagated up as Error
/// return statuses.
///
/// Another common failure is passing in a path that the manifest has labeled as a directory. File
/// paths composed of directory names and file names. Querying for paths that the Manifest has
/// determined previously to be directories will result in Errors.
pub trait Manifest {
    /// Retrieve the FileMetadata that is associated with a path.
    /// Paths that were not set will return None.
    fn get(&self, file_path: &RepoPath) -> Fallible<Option<FileMetadata>>;

    /// Associates a file path with specific file metadata.
    /// A call with a file path that already exists results in an override or the old metadata.
    fn insert(&mut self, file_path: RepoPathBuf, file_metadata: FileMetadata) -> Fallible<()>;

    /// Removes a file from the manifest (equivalent to removing it from the repository).
    /// A call with a file path that does not exist in the manifest is a no-op.
    fn remove(&mut self, file_path: &RepoPath) -> Fallible<()>;

    /// Persists the manifest so that it can be retrieved at a later time. Returns a note
    /// representing the identifier for saved manifest.
    fn flush(&mut self) -> Fallible<Node>;
}

mod file;
pub mod tree;
pub use crate::file::{FileMetadata, FileType};
pub use crate::tree::{diff, DiffEntry, DiffType, Tree, TreeStore};
