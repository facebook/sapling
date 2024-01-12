/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::TryFromIntError;
#[cfg(unix)]
use std::os::unix::prelude::PermissionsExt;
use std::time::SystemTime;

use anyhow::Error;
use anyhow::Result;
use bitflags::bitflags;
use manifest::FileType;
use treestate::filestate::FileStateV2;
use types::RepoPathBuf;
use vfs::VFS;

#[derive(Debug)]
pub(crate) struct File {
    pub path: RepoPathBuf,
    // Outer Option is whether fs_meta is populated. Inner Option is whether file exists.
    pub fs_meta: Option<Option<Metadata>>,
    pub ts_state: Option<FileStateV2>,
}

bitflags! {
    pub(crate) struct MetadataFlags: u8 {
        const IS_SYMLINK = 1 << 0;
        const IS_EXEC = 1 << 1;
        const IS_REGULAR = 1 << 2;
        const IS_DIR = 1 << 3;
        const HAS_MTIME = 1 << 4;
        const HAS_SIZE = 1 << 5;
    }
}

/// Metadata abstracts across the different places file metadata can come from.
#[derive(Debug, Clone)]
pub struct Metadata {
    flags: MetadataFlags,
    size: u64,
    mtime: HgModifiedTime,
    mode: u32,
}

// Watchman sends mode_t even on Windows where they aren't fully
// reflected in libc. Let's just hardcode the values we need.
const S_IFLNK: u32 = 0o120000;
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;

impl Metadata {
    pub fn is_symlink(&self, vfs: &VFS) -> bool {
        vfs.supports_symlinks() && self.flags.intersects(MetadataFlags::IS_SYMLINK)
    }

    pub fn is_executable(&self, vfs: &VFS) -> bool {
        vfs.supports_executables() && self.flags.intersects(MetadataFlags::IS_EXEC)
    }

    pub fn is_file(&self, vfs: &VFS) -> bool {
        if vfs.supports_symlinks() {
            self.flags.intersects(MetadataFlags::IS_REGULAR)
        } else {
            // If symlinks aren't supported, treat symlinks as regular files.
            self.flags
                .intersects(MetadataFlags::IS_REGULAR | MetadataFlags::IS_SYMLINK)
        }
    }

    pub fn is_dir(&self) -> bool {
        self.flags.intersects(MetadataFlags::IS_DIR)
    }

    pub fn len(&self) -> Option<u64> {
        if self.flags.intersects(MetadataFlags::HAS_SIZE) {
            Some(self.size)
        } else {
            None
        }
    }

    pub fn mtime(&self) -> Option<HgModifiedTime> {
        if self.flags.intersects(MetadataFlags::HAS_MTIME) {
            Some(self.mtime)
        } else {
            None
        }
    }

    pub fn from_stat(mode: u32, size: u64, mtime: i64) -> Self {
        let mut flags = MetadataFlags::HAS_SIZE | MetadataFlags::HAS_MTIME;

        if mode & S_IFMT == S_IFLNK {
            flags |= MetadataFlags::IS_SYMLINK;
        } else if mode & 0o111 != 0 {
            flags |= MetadataFlags::IS_EXEC;
        }

        if mode & S_IFMT == S_IFREG {
            flags |= MetadataFlags::IS_REGULAR;
        }

        if mode & S_IFMT == S_IFDIR {
            flags |= MetadataFlags::IS_DIR;
        }

        Self {
            flags,
            size,
            mode,
            mtime: mask_stat_mtime(mtime),
        }
    }

    pub fn mode(&self) -> u32 {
        // Mode may be 0, but that doesn't really matter. Only symlinkness and
        // execness are important.
        let mut mode = self.mode;

        if self.flags.intersects(MetadataFlags::IS_SYMLINK) {
            mode |= S_IFLNK;
        } else if self.flags.intersects(MetadataFlags::IS_EXEC) {
            mode |= 0o111;
        }

        mode
    }
}

impl PartialEq for Metadata {
    fn eq(&self, other: &Self) -> bool {
        self.mode() == other.mode() && self.len() == other.len() && self.mtime() == other.mtime()
    }
}

impl From<FileStateV2> for Metadata {
    fn from(s: FileStateV2) -> Self {
        let mut flags = MetadataFlags::empty();

        let size = match s.size {
            size if size < 0 => 0,
            size => {
                flags |= MetadataFlags::HAS_SIZE;
                size as u64
            }
        };

        let mtime = match s.mtime {
            m if m < 0 => HgModifiedTime(0),
            m => {
                flags |= MetadataFlags::HAS_MTIME;
                HgModifiedTime(m as u64)
            }
        };

        if s.is_symlink() {
            flags |= MetadataFlags::IS_SYMLINK;
        } else {
            flags |= MetadataFlags::IS_REGULAR;
            if s.is_executable() {
                flags |= MetadataFlags::IS_EXEC;
            }
        }

        Self {
            flags,
            size,
            mtime,
            mode: 0,
        }
    }
}

impl From<std::fs::Metadata> for Metadata {
    fn from(m: std::fs::Metadata) -> Self {
        let mut flags = MetadataFlags::HAS_SIZE;

        #[cfg(unix)]
        let mode = m.permissions().mode();

        // This value doesn't really matter - we only care about is_executable
        // and is_symlink in the dirstate. Rust doesn't make something up for
        // us, so put something reasonable in.
        #[cfg(windows)]
        let mode = 0o666;

        if m.is_symlink() {
            flags |= MetadataFlags::IS_SYMLINK;
        } else if m.is_file() {
            flags |= MetadataFlags::IS_REGULAR;
            if mode & 0o111 != 0 {
                flags |= MetadataFlags::IS_EXEC;
            }
        } else if m.is_dir() {
            flags |= MetadataFlags::IS_DIR;
        }

        let mtime = match m.modified() {
            Err(_) => HgModifiedTime(0),
            Ok(mtime) => {
                flags |= MetadataFlags::HAS_MTIME;
                mtime.into()
            }
        };

        Self {
            flags,
            mtime,
            mode,
            size: m.len(),
        }
    }
}

impl From<FileType> for Metadata {
    fn from(ft: FileType) -> Self {
        let flags = match ft {
            FileType::Regular => MetadataFlags::IS_REGULAR,
            FileType::Executable => MetadataFlags::IS_EXEC | MetadataFlags::IS_REGULAR,
            FileType::Symlink => MetadataFlags::IS_SYMLINK,
            FileType::GitSubmodule => MetadataFlags::empty(),
        };

        Self {
            flags,
            mtime: HgModifiedTime(0),
            size: 0,
            mode: 0,
        }
    }
}

/// Represents a file modification time in Mercurial, in seconds since the unix epoch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HgModifiedTime(u64);

impl From<u64> for HgModifiedTime {
    fn from(value: u64) -> Self {
        HgModifiedTime(value)
    }
}

impl From<u32> for HgModifiedTime {
    fn from(value: u32) -> Self {
        HgModifiedTime(value.into())
    }
}

impl TryFrom<HgModifiedTime> for i32 {
    type Error = TryFromIntError;

    fn try_from(value: HgModifiedTime) -> Result<Self, Self::Error> {
        i32::try_from(value.0)
    }
}

// Mask used to make "crazy" mtimes operable. We basically take
// "mtime % 2**31-1". Note that 0x7FFFFFFF is in 2038 - not that far off. We may
// want to reconsider this. https://bz.mercurial-scm.org/show_bug.cgi?id=2608 is
// the original upstream introduction of this workaround.
const CRAZY_MTIME_MASK: i64 = 0x7FFFFFFF;

fn mask_stat_mtime(mtime: i64) -> HgModifiedTime {
    // Handle crazy mtimes by masking into reasonable range. This is what
    // dirstate.py does, so we may get some modicum of compatibility by
    // using the same approach.
    HgModifiedTime((mtime & CRAZY_MTIME_MASK) as u64)
}

impl From<SystemTime> for HgModifiedTime {
    fn from(value: SystemTime) -> Self {
        let signed_epoch = match value.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            // value is before UNIX_EPOCH
            Err(err) => -(err.duration().as_secs() as i64),
        };

        mask_stat_mtime(signed_epoch)
    }
}

impl TryFrom<i32> for HgModifiedTime {
    type Error = Error;
    fn try_from(value: i32) -> Result<Self> {
        Ok(HgModifiedTime(value.try_into()?))
    }
}
