// Copyright Facebook, Inc. 2017
//! File State.

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
    /// Bit flags for a file "state". Certain flags can be used together,
    /// ex. COPIED | ADDED.
    pub struct StateFlags: u16 {
        const ADDED = 1;
        const NORMAL = 2;
        const MERGED = 4;
        const REMOVED = 8;

        /// Explicitly marked as ignored. This means sub-entries with interesting
        /// states (ex. "maybe_changed") are missing from the tree. If the state
        /// changes from "ignored" to not ignored. It requires a plain scan.
        const IGNORED = 16;

        /// Requires a stat check to figure out the state of the file. Use together
        /// with other flag bits.
        const NEED_CHECK = 32;

        const COPIED = 64;
        const OTHERPARENT = 128;
    }
}

impl StateFlags {
    pub fn to_bits(self) -> u16 {
        self.bits
    }
}

/// Information relating to a file in the dirstate, version 2.
/// Unlike V1, the `state` field is no longer a char defined by Mercurial,
/// but a bitflag. It also has a `copied` field.
#[derive(Debug, PartialEq, Clone)]
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
