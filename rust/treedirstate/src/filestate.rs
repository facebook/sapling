// Copyright Facebook, Inc. 2017
//! File State.

use byteorder::{ReadBytesExt, WriteBytesExt};
use errors::*;
use std::io::{Read, Write};
use tree::Storable;
use vlqencoding::{VLQDecode, VLQEncode};

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

impl Storable for FileState {
    /// Write a file entry to the store.
    fn write(&self, mut w: &mut Write) -> Result<()> {
        w.write_u8(self.state)?;
        w.write_vlq(self.mode)?;
        w.write_vlq(self.size)?;
        w.write_vlq(self.mtime)?;
        Ok(())
    }

    /// Read an entry from the store.
    fn read(mut r: &mut Read) -> Result<FileState> {
        let state = r.read_u8()?;
        let mode = r.read_vlq()?;
        let size = r.read_vlq()?;
        let mtime = r.read_vlq()?;
        Ok(FileState {
            state,
            mode,
            size,
            mtime,
        })
    }
}
