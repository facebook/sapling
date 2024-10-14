/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;

/// Used to signal the type of element in a directory: file or directory.
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
#[serde(rename_all = "snake_case")]
pub enum TreeItemFlag {
    File(FileType),
    Directory,
}

/// The types of files (leaf nodes in a tree).
///
/// The type needs to round-trip tree serialization.
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Copy,
    Debug,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash
)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FileType {
    /// Regular files.
    #[default]
    Regular,
    /// Executable files. Like Regular files but with the executable flag set.
    Executable,
    /// Symlinks. Their targets are not limited to repository paths. They can point anywhere.
    Symlink,
    /// Git submodule. It's up to the higher layer to decide what to do with them.
    GitSubmodule,
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for FileType {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        g.choose(&[FileType::Regular, FileType::Executable, FileType::Symlink])
            .unwrap()
            .to_owned()
    }
}
