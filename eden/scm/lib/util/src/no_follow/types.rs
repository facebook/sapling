/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Small metadata type for no-follow filesystem operations.
//!
//! This intentionally exposes only the subset of `std::fs::Metadata` needed by
//! current callers. `std::fs::Metadata` cannot be constructed portably from
//! fd-relative platform APIs.

use std::io;
use std::str::FromStr;
use std::time::SystemTime;

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiteMetadata {
    pub(super) mode: u32,
    pub(super) size: u64,
    pub(super) accessed: SystemTime,
    pub(super) modified: SystemTime,
    pub(super) ctime: SystemTime,
    pub(super) dev: u64,
    pub(super) ino: u64,
    pub(super) nlink: u64,
    pub(super) uid: u32,
    pub(super) gid: u32,
}

bitflags::bitflags! {
    /// Cross-platform open flags for [`NoFollowRoot`](crate::no_follow::NoFollowRoot).
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct OpenFlags: u32 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const CREATE = 1 << 2;
        const CREATE_NEW = 1 << 3;
        const TRUNCATE = 1 << 4;
        const APPEND = 1 << 5;
    }
}

impl FromStr for OpenFlags {
    type Err = io::Error;

    fn from_str(mode: &str) -> io::Result<Self> {
        let mut flags = Self::empty();
        for ch in mode.chars() {
            match ch {
                'r' => flags |= Self::READ,
                'w' => flags |= Self::WRITE | Self::CREATE | Self::TRUNCATE,
                'a' => flags |= Self::APPEND | Self::CREATE,
                'c' => flags |= Self::CREATE,
                'x' => flags |= Self::WRITE | Self::CREATE_NEW,
                'b' => {}
                '+' => {
                    if mode.contains('r') {
                        flags |= Self::WRITE
                    } else {
                        flags |= Self::READ;
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown no-follow open mode: {:?}", mode),
                    ));
                }
            }
        }
        if flags.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty no-follow open mode",
            ));
        }
        Ok(flags)
    }
}

impl OpenFlags {
    /// Is this `OpenFlags` creating a file?
    /// If so, its ancestor directories might need to be created automatically.
    pub fn creates_file(self) -> bool {
        self.intersects(Self::CREATE | Self::CREATE_NEW)
    }
}

impl LiteMetadata {
    pub fn mode(&self) -> u32 {
        self.mode
    }

    pub fn len(&self) -> u64 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn accessed(&self) -> SystemTime {
        self.accessed
    }

    pub fn atime(&self) -> SystemTime {
        self.accessed()
    }

    pub fn modified(&self) -> SystemTime {
        self.modified
    }

    pub fn mtime(&self) -> SystemTime {
        self.modified()
    }

    pub fn ctime(&self) -> SystemTime {
        self.ctime
    }

    pub fn dev(&self) -> u64 {
        self.dev
    }

    pub fn ino(&self) -> u64 {
        self.ino
    }

    pub fn nlink(&self) -> u64 {
        self.nlink
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }

    pub fn is_executable(&self) -> bool {
        self.mode & 0o100 != 0
    }
}

#[cfg(windows)]
pub(crate) fn file_mode(permissions: u32) -> u32 {
    S_IFREG | permissions
}

#[cfg(windows)]
pub(crate) fn dir_mode(permissions: u32) -> u32 {
    S_IFDIR | permissions
}

#[cfg(windows)]
pub(crate) fn symlink_mode(permissions: u32) -> u32 {
    S_IFLNK | permissions
}
