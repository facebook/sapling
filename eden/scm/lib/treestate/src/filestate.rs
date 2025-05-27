/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! File State.

use bitflags::bitflags;

/// Information relating to a file in the dirstate.
#[derive(Debug, PartialEq, Copy, Clone)]
pub struct FileState {
    /// State of the file, as recorded by Mercurial.  Mercurial uses a single character to
    /// represent the current state of the file.  Only a single byte is used in the file, so only
    /// ASCII characters are valid here.
    pub state: u8,

    /// Mode (permissions) mask for the file.
    pub mode: u32,

    /// Size of the file.  Mercurial uses negative sizes for special values, so this must be
    /// signed.
    pub size: i32,

    /// Modification time of the file.
    pub mtime: i32,
}

impl FileState {
    pub fn new(state: u8, mode: u32, size: i32, mtime: i32) -> FileState {
        FileState {
            state,
            mode,
            size,
            mtime,
        }
    }
}

bitflags! {
    /// Bit flags for a file "state". Certain flags can be used together.
    ///
    /// Mapping to some Mercurial's concepts:
    ///
    /// |           | EXIST_P1 | EXIST_P2 | EXIST_NEXT | IGNORED |
    /// | added     | no       | no       | yes        | ?       |
    /// | merge     | yes      | yes      | yes        | ?       |
    /// | normal    | yes      | no       | yes        | ?       |
    /// | normal    | no       | yes      | yes        | ?       |
    /// | removed   | either one is yes   | no         | ?       |
    /// | untracked | no       | no       | no         | no      |
    /// | ignored   | no       | no       | no         | yes     |
    #[cfg_attr(test, derive(Default))]
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct StateFlags: u16 {
        /// Exist in the first working parent.
        const EXIST_P1 = 1;

        /// Exist in a non-first working parent.
        const EXIST_P2 = 2;

        /// Will exist in the next commit.
        const EXIST_NEXT = 4;

        /// Explicitly marked as ignored.
        const IGNORED = 8;

        /// Known possibly changed. Need stat check.
        ///
        /// For non-watchman case, this is a quick way to get all mtime < 0 entries. aka. for
        /// calculating non-normal set quickly.
        ///
        /// For watchman case, this also includes untracked files and normal files with mtime >= 0,
        /// that are known changed during the last watchman check. Combined with a new watchman
        /// query since the recorded watchman clock, the caller can figure out all files that are
        /// possibly changed, and ignore files outside that list.
        const NEED_CHECK = 16;

        /// Marked as copied from another path.
        const COPIED = 32;
    }
}

impl StateFlags {
    /// Convenience mask representing whether a file is tracked in either parent
    /// commit or next commit.
    pub const TRACKED: Self = Self::EXIST_P1.union(Self::EXIST_P2).union(Self::EXIST_NEXT);

    pub fn to_bits(self) -> u16 {
        self.bits()
    }

    pub fn is_tracked(&self) -> bool {
        self.intersects(Self::TRACKED)
    }
}

/// Information relating to a file in the dirstate, version 2.
/// Unlike V1, the `state` field is no longer a char defined by Mercurial,
/// but a bitflag. It also has a `copied` field.
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(test, derive(Default))]
pub struct FileStateV2 {
    /// Mode (permissions) mask for the file.
    pub mode: u32,

    /// Size of the file.  Mercurial uses negative sizes for special values, so this must be
    /// signed.
    pub size: i32,

    /// Modification time of the file.
    pub mtime: i32,

    /// State of the file.
    pub state: StateFlags,

    /// Path copied from.
    pub copied: Option<Box<[u8]>>,
}

impl FileStateV2 {
    pub fn is_executable(&self) -> bool {
        // Symlinks show as executable, but don't be fooled. "executable" and
        // "symlink" are mutually exclusive in the manifest, so it is just
        // confusing if we have dirstate entries that claim to be both.
        !self.is_symlink() && self.mode & 0o100 == 0o100
    }

    pub fn is_symlink(&self) -> bool {
        self.mode & 0o120000 == 0o120000
    }
}

#[cfg(test)]
impl rand::distributions::Distribution<FileStateV2> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> FileStateV2 {
        let mode = rng.r#gen();
        let size = rng.r#gen();
        let mtime = rng.r#gen();
        let state = StateFlags::from_bits_truncate(rng.r#gen());
        let copied = if state.contains(StateFlags::COPIED) {
            Some(b"copied_source".to_vec().into_boxed_slice())
        } else {
            None
        };
        FileStateV2 {
            mode,
            size,
            mtime,
            state,
            copied,
        }
    }
}
