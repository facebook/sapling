/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

/// A single path element.
typedef binary MPathElement (
  rust.newtype,
  rust.type = "smallvec::SmallVec<[u8; 24]>",
)

/// A path.  Paths are stored as lists of elements so that the sort order of
/// paths is the same as that of tree traversal.
typedef list<MPathElement> MPath (rust.newtype)

/// A path that is known not to be the root.
typedef MPath NonRootMPath (rust.newtype)

/// A path this is known to be a file or directory (or the root).  This is used
/// in Mercurial-based types where files and directories are treated
/// differently.
union RepoPath {
  # Thrift language doesn't support void here, so put a dummy bool
  1: bool RootPath;
  2: NonRootMPath DirectoryPath;
  3: NonRootMPath FilePath;
}
