// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use types::{Node, RepoPathBuf};

use crate::tree::Link;

/// A file entry in a tree manifest.
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

    /// Create a file record for a `Link`, failing if the link
    /// refers to a directory rather than a file.
    pub fn from_link(link: &Link, path: RepoPathBuf) -> Option<Self> {
        match link {
            Link::Leaf(meta) => Some(Self::new(path, *meta)),
            _ => None,
        }
    }
}

impl From<(RepoPathBuf, FileMetadata)> for File {
    fn from((path, meta): (RepoPathBuf, FileMetadata)) -> Self {
        Self { path, meta }
    }
}

/// The contents of the Manifest for a file.
/// * node: used to determine the revision of the file in the repository.
/// * file_type: determines the type of the file.
#[derive(Clone, Copy, Debug, Default, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct FileMetadata {
    pub node: Node,
    pub file_type: FileType,
}

/// The types of files that are supported.
///
/// The debate here is whether to use Regular { executable: bool } or an Executable variant.
/// Technically speaking executable files are regular files. There is no big difference in terms
/// of the mechanics between the two approaches. The approach using an Executable is more readable
/// so that is what we have now.
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum FileType {
    /// Regular files.
    Regular,
    /// Executable files. Like Regular files but with the executable flag set.
    Executable,
    /// Symlinks. Their targets are not limited to repository paths. They can point anywhere.
    Symlink,
}

impl Default for FileType {
    fn default() -> Self {
        FileType::Regular
    }
}

impl FileMetadata {
    pub fn new(node: Node, file_type: FileType) -> Self {
        Self { node, file_type }
    }

    /// Creates `FileMetadata` with file_type set to `FileType::Regular`.
    pub fn regular(node: Node) -> Self {
        Self::new(node, FileType::Regular)
    }

    /// Creates `FileMetadata` with file_type set to `FileType::Executable`.
    pub fn executable(node: Node) -> Self {
        Self::new(node, FileType::Executable)
    }

    /// Creates `FileMetadata` with file_type set to `FileType::Symlink`.
    pub fn symlink(node: Node) -> Self {
        Self::new(node, FileType::Symlink)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for FileType {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        g.choose(&[FileType::Regular, FileType::Executable, FileType::Symlink])
            .unwrap()
            .to_owned()
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for FileMetadata {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let node = Node::arbitrary(g);
        let file_type = FileType::arbitrary(g);
        FileMetadata::new(node, file_type)
    }
}
