/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::anyhow;
use anyhow::Result;
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

    pub fn manifest_suffix(&self) -> Result<&'static [u8]> {
        match self {
            Type::Tree => Ok(b"t"),
            Type::File(FileType::Symlink) => Ok(b"l"),
            Type::File(FileType::Executable) => Ok(b"x"),
            Type::File(FileType::Regular) => Ok(b""),
            Type::File(FileType::GitSubmodule) => Err(anyhow!("Git submodules not supported")),
        }
    }

    pub fn augmented_manifest_suffix(&self) -> Result<&'static [u8]> {
        match self {
            Type::Tree => Ok(b"t"),
            Type::File(FileType::Symlink) => Ok(b"l"),
            Type::File(FileType::Executable) => Ok(b"x"),
            Type::File(FileType::Regular) => Ok(b"r"),
            Type::File(FileType::GitSubmodule) => Err(anyhow!("Git submodules not supported")),
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
