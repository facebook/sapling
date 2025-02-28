/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! manifest - The contents of the repository at a specific commit.
//!
//! The history of the repository is recorded in the commit graph. Each commit has a manifest
//! associated with it. The manifest specifies the revision for all the files available in the
//! repository. The file path and file revision are then used to retrieve the contents of the
//! file thus achieving the reconstruction of the entire repository state.

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;

use anyhow::Result;
use pathmatcher::Matcher;
pub use types::tree::FileType;
use types::HgId;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

/// Manifest describes a mapping between file path ([`String`]) and file metadata ([`FileMetadata`]).
/// Fundamentally it is just a Map<file_path, file_metadata>.
///
/// It can be assumed that Manifest interacts with an underlying store for persistence. These
/// interactions may fail due to a variety of reasons. Such failures will be propagated up as Error
/// return statuses.
///
/// Another common failure is passing in a path that the manifest has labeled as a directory. File
/// paths composed of directory names and file names. Querying for paths that the Manifest has
/// determined previously to be directories will result in Errors.
// TODO: Add method for batch modification, takes iterator of added, removed, changed, or
// maybe (path, Option<FileMetadata>) where None signals removal.
// TODO: A batch API allows us to move to having all nodes have a computed hash without losing
// performance. It also allows us to get rid of the flush method.
pub trait Manifest {
    /// Inspects the manifest for the given path. Returns available metadata.
    /// If the path is pointing to an file then Some(FsNodeMetadata::File) is returned with the
    /// file_metadata associated with the file. If the path is pointing to a directory then
    /// Some(FsNodeMetadata::Directory) is returned. If the path is not found then None is
    /// returned.
    // TODO: add default implementation
    fn get(&self, path: &RepoPath) -> Result<Option<FsNodeMetadata>>;

    fn get_ignore_case(&self, path: &RepoPath) -> Result<Option<FsNodeMetadata>>;

    /// Lists the immediate contents of directory in a manifest (non-recursive).
    /// Given a path, the manifest will return:
    /// * List::NotFound when the path is not present in the manifest
    /// * List::File when the path points to a file
    /// * List::Directory when the path points to a directory
    ///    wraps the names of the files and directories in this directory
    // TODO: add default implementation
    fn list(&self, path: &RepoPath) -> Result<List>;

    /// Associates a file path with specific file metadata.
    /// A call with a file path that already exists results in an override or the old metadata.
    fn insert(&mut self, file_path: RepoPathBuf, file_metadata: FileMetadata) -> Result<()>;

    /// Removes a file from the manifest (equivalent to removing it from the repository).
    /// A call with a file path that does not exist in the manifest is a no-op.
    fn remove(&mut self, file_path: &RepoPath) -> Result<Option<FileMetadata>>;

    /// Persists the manifest so that it can be retrieved at a later time. Returns a note
    /// representing the identifier for saved manifest.
    fn flush(&mut self) -> Result<HgId>;

    /// Retrieve the FileMetadata associated with a path.
    /// Paths that were not set will return None.
    fn get_file(&self, file_path: &RepoPath) -> Result<Option<FileMetadata>> {
        let result = self.get(file_path)?.and_then(|fs_hgid| match fs_hgid {
            FsNodeMetadata::File(file_metadata) => Some(file_metadata),
            FsNodeMetadata::Directory(_) => None,
        });
        Ok(result)
    }

    fn contains_file(&self, file_path: &RepoPath) -> Result<bool> {
        Ok(self.get_file(file_path)?.is_some())
    }

    /// Returns an iterator over all the files in the Manifest that satisfy the given Matcher.
    fn files<'a, M: 'static + Matcher + Sync + Send>(
        &'a self,
        matcher: M,
    ) -> Box<dyn Iterator<Item = Result<File>> + 'a>;

    /// Returns the number of files in the Manifest that satisfy the given Matcher.
    fn count_files<'a, M: 'static + Matcher + Sync + Send>(&'a self, matcher: M) -> Result<u64> {
        self.files(matcher).try_fold(0, |acc, result| {
            let _ = result?;
            Ok(acc + 1)
        })
    }

    /// Returns an iterator over all directories found in the paths of the files in the Manifest
    /// that satisfy the given Matcher.
    // TODO: add default implementation
    fn dirs<'a, M: 'static + Matcher + Sync + Send>(
        &'a self,
        matcher: M,
    ) -> Box<dyn Iterator<Item = Result<Directory>> + 'a>;

    /// Returns an iterator of all the differences in files between two Manifest instances of the
    /// same type.
    // TODO: add default implementation
    fn diff<'a, M: 'static + Matcher + Sync + Send>(
        &'a self,
        other: &'a Self,
        matcher: M,
    ) -> Result<Box<dyn Iterator<Item = Result<DiffEntry>> + 'a>>;
}

/// The result of a list operation. Given a path, the manifest will return:
/// * List::NotFound when the path is not present in the manifest
/// * List::File when the path points to a file
/// * List::Directory when the path points to a directory
// TODO: add FileMetadata to File and "DirectoryMetadata" to Directory
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum List {
    NotFound,
    File,
    Directory(Vec<(PathComponentBuf, FsNodeMetadata)>),
}

/// FsNodeMetadata short for file system node.
/// The manifest tracks a list of files. However file systems are hierarchical structures
/// composed of directories and files at the end. For different operations it is useful to have
/// a representation for file or directory. A good example is listing a directory. This structure
/// helps us represent that notion.
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum FsNodeMetadata {
    File(FileMetadata),
    Directory(Option<HgId>),
}

/// A directory entry in a manifest.
///
/// Consists of the full path to the directory. Directories may or may not be assigned
/// identifiers. When an identifier is available it is also returned.
// TODO: Move hgid to a new DirectoryMetadata struct.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Directory {
    pub path: RepoPathBuf,
    pub hgid: Option<HgId>,
}

impl Directory {
    pub fn new(path: RepoPathBuf, hgid: Option<HgId>) -> Self {
        Self { path, hgid }
    }
}

/// A file entry in a manifest.
///
/// Consists of the full path to the file along with the associated file metadata.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct File {
    pub path: RepoPathBuf,
    pub meta: FileMetadata,
}

impl File {
    pub fn new(path: RepoPathBuf, meta: FileMetadata) -> Self {
        Self { path, meta }
    }
}

/// The contents of the Manifest for a file.
#[derive(Clone, Copy, Debug, Default, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct FileMetadata {
    /// Used to determine the revision of the file in the repository.
    pub hgid: HgId,
    /// The type of the file.
    pub file_type: FileType,
    /// Only used during manifest diff. It instructs diff to ignore this file unless it
    /// conflicts with something on the other side.
    pub ignore_unless_conflict: bool,
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for FileMetadata {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            hgid: HgId::arbitrary(g),
            file_type: FileType::arbitrary(g),
            ignore_unless_conflict: false,
        }
    }
}

impl FileMetadata {
    pub fn new(hgid: HgId, file_type: FileType) -> Self {
        Self {
            hgid,
            file_type,
            ignore_unless_conflict: false,
        }
    }

    /// Creates `FileMetadata` with file_type set to `FileType::Regular`.
    pub fn regular(hgid: HgId) -> Self {
        Self::new(hgid, FileType::Regular)
    }

    /// Creates `FileMetadata` with file_type set to `FileType::Executable`.
    pub fn executable(hgid: HgId) -> Self {
        Self::new(hgid, FileType::Executable)
    }

    /// Creates `FileMetadata` with file_type set to `FileType::Symlink`.
    pub fn symlink(hgid: HgId) -> Self {
        Self::new(hgid, FileType::Symlink)
    }
}

/// Represents a file that is different between two tree manifests.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct DiffEntry {
    pub path: RepoPathBuf,
    pub diff_type: DiffType,
}

impl DiffEntry {
    pub fn new(path: RepoPathBuf, diff_type: DiffType) -> Self {
        DiffEntry { path, diff_type }
    }

    pub fn left(file: File) -> Self {
        Self::new(file.path, DiffType::LeftOnly(file.meta))
    }

    pub fn right(file: File) -> Self {
        Self::new(file.path, DiffType::RightOnly(file.meta))
    }

    pub fn changed(left: File, right: File) -> Self {
        debug_assert!(left.path == right.path);
        Self::new(left.path, DiffType::Changed(left.meta, right.meta))
    }
}

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum DiffType {
    LeftOnly(FileMetadata),
    RightOnly(FileMetadata),
    Changed(FileMetadata, FileMetadata),
}

impl DiffType {
    /// Returns the metadata of the file in the left manifest when it exists.
    pub fn left(&self) -> Option<FileMetadata> {
        match self {
            DiffType::LeftOnly(left_metadata) => Some(*left_metadata),
            DiffType::RightOnly(_) => None,
            DiffType::Changed(left_metadata, _) => Some(*left_metadata),
        }
    }

    /// Returns the metadata of the file in the right manifest when it exists.
    pub fn right(&self) -> Option<FileMetadata> {
        match self {
            DiffType::LeftOnly(_) => None,
            DiffType::RightOnly(right_metadata) => Some(*right_metadata),
            DiffType::Changed(_, right_metadata) => Some(*right_metadata),
        }
    }
}
