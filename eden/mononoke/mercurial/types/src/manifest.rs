/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use mononoke_types::FileType;
use serde_derive::Serialize;

/// Type of an Entry
///
/// File and Executable are identical - they both represent files containing arbitrary content.
/// The only difference is that the Executables are created with executable permission when
/// checked out.
///
/// Symlink is also the same as File, but the content of the file is interpolated into a path
/// being traversed during lookup.
///
/// Tree is a reference to another Manifest (directory-like) object.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub enum Type {
    File(FileType),
    Tree,
}

impl Type {
    #[inline]
    pub fn is_tree(&self) -> bool {
        self == &Type::Tree
    }

    pub fn manifest_suffix(&self) -> &'static str {
        // It's a little weird that this returns a Unicode string and not a bytestring, but that's
        // what callers demand.
        match self {
            Type::Tree => "t",
            Type::File(FileType::Symlink) => "l",
            Type::File(FileType::Executable) => "x",
            Type::File(FileType::Regular) => "",
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Type::Tree => write!(f, "tree"),
            Type::File(ft) => write!(f, "{}", ft),
        }
    }
}
