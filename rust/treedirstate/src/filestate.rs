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
