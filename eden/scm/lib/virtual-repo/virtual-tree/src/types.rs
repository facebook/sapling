/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
pub use std::num::NonZeroU64;

pub use crate::std_ext::LosslessShl;

/// Identifies a tree.
/// TreeId > 0.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TreeId(pub NonZeroU64);

/// Decides the file content.
/// BlobId > 0.
/// Local to a path.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlobId(pub NonZeroU64);

/// Defines the name of a tree item.
/// Local to a tree path.
/// NameId > 0.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameId(pub NonZeroU64);

/// A "suggested random seed" for names or file contents inside a tree. The seed
/// should remain the same for the same path. For example, in two different
/// commits X and Y, the tree of path dir1/dir2 might have different
/// [`TreeId`]`s but they should share the same seed so the file names and
/// contents remain stable between commit X and Y. It's also valid to use
/// a constant for all trees in a repo.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct TreeSeed(pub u64);

/// Defines the content of a tree item.
/// Lower 2-bit stores whether this is a tree (0), or a regular file (1),
/// executable (2) or symlink (3).
/// Local to a (tree or file) path.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentId(pub NonZeroU64);

/// Decoded `ContentId`.
pub enum TypedContentId {
    Tree(TreeId),
    File(BlobId, FileMode),
    /// Useful for certain logic (ex. split_changes) without introducing another
    /// layer of `Option`.
    Absent,
}

#[derive(Debug, Copy, Clone)]
pub enum FileMode {
    Regular,
    Executable,
    Symlink,
}

impl ContentId {
    pub const ABSENT: ContentId = ContentId(NonZeroU64::MAX);

    pub fn is_absent(self) -> bool {
        self == Self::ABSENT
    }
}

impl From<ContentId> for TypedContentId {
    fn from(value: ContentId) -> Self {
        if value.is_absent() {
            return Self::Absent;
        }
        let flag = value.0.get() & 0b11;
        let value = NonZeroU64::new(value.0.get() >> 2).unwrap();
        match flag {
            0 => Self::Tree(TreeId(value)),
            1 => Self::File(BlobId(value), FileMode::Regular),
            2 => Self::File(BlobId(value), FileMode::Executable),
            _ => Self::File(BlobId(value), FileMode::Symlink),
        }
    }
}

impl From<TypedContentId> for ContentId {
    fn from(value: TypedContentId) -> Self {
        let new_value = match value {
            TypedContentId::Tree(tree_id) => tree_id.0.get().lossless_shl(2),
            TypedContentId::File(blob_id, file_mode) => {
                let flag = match file_mode {
                    FileMode::Regular => 1,
                    FileMode::Executable => 2,
                    FileMode::Symlink => 3,
                };
                blob_id.0.get().lossless_shl(2) | flag
            }
            TypedContentId::Absent => return ContentId::ABSENT,
        };
        Self(NonZeroU64::new(new_value).unwrap())
    }
}

impl fmt::Debug for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let typed = TypedContentId::from(*self);
        match typed {
            TypedContentId::Tree(tree_id) => write!(f, "Tree {tree_id:?}"),
            TypedContentId::File(blob_id, file_mode) => {
                write!(f, "File {blob_id:?} {file_mode:?}")
            }
            TypedContentId::Absent => write!(f, "Absent"),
        }
    }
}

impl fmt::Debug for TreeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

impl fmt::Debug for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "B{}", self.0)
    }
}

impl fmt::Debug for NameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "N{}", self.0)
    }
}

pub type ReadTreeIter<'a> = Box<dyn Iterator<Item = (NameId, ContentId)> + 'a>;

/// Abstraction for tree providers.
/// Use integers instead of byte slices for efficiency and easy bit operations.
pub trait VirtualTreeProvider: Send + Sync + 'static {
    /// List items in a tree. `NameId` should be sorted.
    fn read_tree<'a>(&'a self, tree_id: TreeId) -> ReadTreeIter<'a>;

    /// A "random seed" associated with the tree to randomize file names and
    /// contents within the tree. It is usually an approximate of the actual
    /// path of the tree so file contents remain stable for the tree.
    fn get_tree_seed(&self, tree_id: TreeId) -> TreeSeed;

    /// Get the length of root trees.
    fn root_tree_len(&self) -> usize;

    /// Get the root tree id by index. `index` should be in the range of `0..root_tree_len()`.
    fn root_tree_id(&self, index: usize) -> TreeId;
}
