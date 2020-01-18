/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::IoResultExt;
use crate::utils::{self, atomic_write, xxhash};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use vlqencoding::{VLQDecode, VLQEncode};

/// Metadata about index names, logical [`Log`] and [`Index`] file lengths.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LogMetadata {
    /// Length of the primary log file.
    pub(crate) primary_len: u64,

    /// Lengths of index files. Name => Length.
    pub(crate) indexes: BTreeMap<String, u64>,

    /// Used to detect non-append-only changes.
    /// Conceptually similar to "create time".
    pub(crate) epoch: u64,
}

impl LogMetadata {
    const HEADER: &'static [u8] = b"meta\0";

    /// Read metadata from a reader.
    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut header = vec![0; Self::HEADER.len()];
        reader.read_exact(&mut header)?;
        if header != Self::HEADER {
            let msg = "invalid metadata header";
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
        }

        let hash = reader.read_vlq()?;
        let buf_len = reader.read_vlq()?;

        let mut buf = vec![0; buf_len];
        reader.read_exact(&mut buf)?;

        if xxhash(&buf) != hash {
            let msg = "metadata integrity check failed";
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
        }

        let mut reader = Cursor::new(buf);
        let primary_len = reader.read_vlq()?;
        let index_count: usize = reader.read_vlq()?;
        let mut indexes = BTreeMap::new();
        for _ in 0..index_count {
            let name_len = reader.read_vlq()?;
            let mut name = vec![0; name_len];
            reader.read_exact(&mut name)?;
            let name = String::from_utf8(name).map_err(|_e| {
                let msg = "non-utf8 index name";
                io::Error::new(io::ErrorKind::InvalidData, msg)
            })?;
            let len = reader.read_vlq()?;
            indexes.insert(name, len);
        }

        // 'epoch' is optional - it does not exist in a previous serialization
        // format. So not being able to read it (because EOF) is not fatal.
        let epoch = reader.read_vlq().unwrap_or_default();

        Ok(Self {
            primary_len,
            indexes,
            epoch,
        })
    }

    /// Write metadata to a writer.
    pub fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let mut buf = Vec::new();
        buf.write_vlq(self.primary_len)?;
        buf.write_vlq(self.indexes.len())?;
        for (name, len) in self.indexes.iter() {
            let name = name.as_bytes();
            buf.write_vlq(name.len())?;
            buf.write_all(name)?;
            buf.write_vlq(*len)?;
        }
        buf.write_vlq(self.epoch)?;
        writer.write_all(Self::HEADER)?;
        writer.write_vlq(xxhash(&buf))?;
        writer.write_vlq(buf.len())?;
        writer.write_all(&buf)?;

        Ok(())
    }

    /// Read metadata from a file.
    pub fn read_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = fs::OpenOptions::new().read(true).open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let mut cur = Cursor::new(buf);
        Self::read(&mut cur)
    }

    /// Atomically write metadata to a file.
    pub fn write_file<P: AsRef<Path>>(&self, path: P, fsync: bool) -> crate::Result<()> {
        let mut buf = Vec::new();
        self.write(&mut buf).infallible()?;
        atomic_write(path, &buf, fsync)?;
        Ok(())
    }

    /// Create a new LogMetadata that matches the primary length with
    /// empty indexes.
    /// The caller must make sure the primary log is consistent (exists,
    /// and covered the length).
    pub(crate) fn new_with_primary_len(len: u64) -> Self {
        Self {
            primary_len: len,
            indexes: BTreeMap::new(),
            epoch: utils::epoch(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use tempfile::tempdir;

    quickcheck! {
        fn test_roundtrip_meta(primary_len: u64, indexes: BTreeMap<String, u64>, epoch: u64) -> bool {
            let mut buf = Vec::new();
            let meta = LogMetadata { primary_len, indexes, epoch };
            meta.write(&mut buf).expect("write");
            let mut cur = Cursor::new(buf);
            let meta_read = LogMetadata::read(&mut cur).expect("read");
            meta_read == meta
        }

        fn test_roundtrip_meta_file(primary_len: u64, indexes: BTreeMap<String, u64>, epoch: u64) -> bool {
            let dir = tempdir().unwrap();
            let meta = LogMetadata { primary_len, indexes, epoch };
            let path = dir.path().join("meta");
            meta.write_file(&path, false).expect("write_file");
            let meta_read = LogMetadata::read_file(&path).expect("read_file");
            meta_read == meta
        }

    }
}
