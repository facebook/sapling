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
    /// Inspects the manifest for the given path.
    /// If the path is pointing to an file then Some(FsNode::File) is returned with then
    /// file_metadata associated with the file. If the path is poitning to a directory then
    /// Some(FsNode::Directory) is returned. If the path is not found then None is returned.
    fn get(&self, path: &RepoPath) -> Fallible<Option<FsNode>>;

    /// Associates a file path with specific file metadata.
    /// A call with a file path that already exists results in an override or the old metadata.
    fn insert(&mut self, file_path: RepoPathBuf, file_metadata: FileMetadata) -> Fallible<()>;

    /// Removes a file from the manifest (equivalent to removing it from the repository).
    /// A call with a file path that does not exist in the manifest is a no-op.
    fn remove(&mut self, file_path: &RepoPath) -> Fallible<Option<FileMetadata>>;

    /// Persists the manifest so that it can be retrieved at a later time. Returns a note
    /// representing the identifier for saved manifest.
    fn flush(&mut self) -> Fallible<Node>;

    /// Retrieve the FileMetadata that is associated with a path.
    /// Paths that were not set will return None.
    fn get_file(&self, file_path: &RepoPath) -> Fallible<Option<FileMetadata>> {
        let result = self.get(file_path)?.and_then(|fs_node| match fs_node {
            FsNode::File(file_metadata) => Some(file_metadata),
            FsNode::Directory => None,
        });
        Ok(result)
    }
}

/// FsNode short for file system node.
/// The manifest tracks a list of files. However file systems are hierarchical structures
/// composed of directories and files at the end. For different operations it is useful to have
/// a representation for file or directory. A good example is listing a directory. This structure
/// helps us represent that notion.
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum FsNode {
    Directory,
    File(FileMetadata),
}

mod file;
pub mod tree;
pub use crate::file::{FileMetadata, FileType};
pub use crate::tree::{
    compat_subtree_diff, diff, BfsDiff, Diff, DiffEntry, DiffType, Tree, TreeStore,
};
