/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::errors::IoResultExt;
use crate::utils;
use crate::utils::atomic_read;
use crate::utils::atomic_write;
use crate::utils::xxhash;

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
    /// Read metadata from a reader.
    pub fn read<R: Read>(mut reader: R) -> io::Result<Self> {
        let header = HeaderVersion::from_reader(&mut reader)?;
        let hash: u64 = match header {
            HeaderVersion::V0 => reader.read_vlq()?,
            HeaderVersion::V1 => reader.read_u64::<LittleEndian>()?,
        };
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
        let header = if cfg!(test) {
            HeaderVersion::V1
        } else {
            HeaderVersion::V0
        };
        self.write_using_header(writer, header)
    }

    /// Write using specified header. Used by tests.
    fn write_using_header<W: Write>(
        &self,
        writer: &mut W,
        header: HeaderVersion,
    ) -> io::Result<()> {
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
        writer.write_all(header.to_bytes())?;
        match header {
            HeaderVersion::V1 => writer.write_u64::<LittleEndian>(xxhash(&buf))?,
            HeaderVersion::V0 => writer.write_vlq(xxhash(&buf))?,
        }
        writer.write_vlq(buf.len())?;
        writer.write_all(&buf)?;

        Ok(())
    }

    /// Read metadata from a file.
    pub fn read_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let path = path.as_ref();
        let buf = atomic_read(path).context(path, "when reading LogMetadata")?;
        Self::read(&buf[..]).context(path, || {
            format!("when parsing LogMetadata (content: {:?})", &buf)
        })
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
            epoch: utils::rand_u64(),
        }
    }

    /// Test if two Metadata is compatible, aka. having the same length
    /// and epoch.
    pub(crate) fn is_compatible_with(&self, other: &Self) -> bool {
        self.primary_len == other.primary_len && self.epoch == other.epoch
    }
}

#[derive(Copy, Clone, Debug)]
enum HeaderVersion {
    V0,

    // V1: xxhash uses fixed 8 bytes instead of vlq.
    V1,
}

impl HeaderVersion {
    const HEADER_V0: &'static [u8] = b"meta\0";
    const HEADER_V1: &'static [u8] = b"meta\x01";

    fn from_reader(reader: &mut dyn Read) -> io::Result<Self> {
        assert_eq!(Self::HEADER_V0.len(), Self::HEADER_V0.len());
        let mut header = vec![0; Self::HEADER_V0.len()];
        reader.read_exact(&mut header)?;
        if header == Self::HEADER_V1 {
            Ok(Self::V1)
        } else if header == Self::HEADER_V0 {
            Ok(Self::V0)
        } else {
            let msg = "invalid metadata header";
            Err(io::Error::new(io::ErrorKind::InvalidData, msg))
        }
    }

    fn to_bytes(&self) -> &[u8] {
        match self {
            Self::V0 => Self::HEADER_V0,
            Self::V1 => Self::HEADER_V1,
        }
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use tempfile::tempdir;

    use super::*;

    quickcheck! {
        fn test_roundtrip_meta(primary_len: u64, indexes: BTreeMap<String, u64>, epoch: u64) -> bool {
            let mut buf = Vec::new();
            let meta = LogMetadata { primary_len, indexes, epoch,  };
            meta.write(&mut buf).expect("write");
            let mut cur = Cursor::new(buf);
            let meta_read = LogMetadata::read(&mut cur).expect("read");
            meta_read == meta
        }

        fn test_roundtrip_meta_v0(primary_len: u64, indexes: BTreeMap<String, u64>, epoch: u64) -> bool {
            let mut buf = Vec::new();
            let meta = LogMetadata { primary_len, indexes, epoch,  };
            meta.write_using_header(&mut buf, HeaderVersion::V0).expect("write");
            let mut cur = Cursor::new(buf);
            let meta_read = LogMetadata::read(&mut cur).expect("read");
            meta_read == meta
        }

        fn test_roundtrip_meta_file(primary_len: u64, indexes: BTreeMap<String, u64>, epoch: u64) -> bool {
            let dir = tempdir().unwrap();
            let meta = LogMetadata { primary_len, indexes, epoch,  };
            let path = dir.path().join("meta");
            meta.write_file(&path, false).expect("write_file");
            let meta_read = LogMetadata::read_file(&path).expect("read_file");
            meta_read == meta
        }
    }

    #[test]
    fn test_read_file_includes_file_content_on_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta");
        let meta = LogMetadata {
            primary_len: 1,
            indexes: Default::default(),
            epoch: 42,
        };
        let mut buf: Vec<u8> = Vec::new();
        meta.write(&mut buf).unwrap();
        *buf.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &buf).unwrap();
        let err = LogMetadata::read_file(&path).unwrap_err();
        let content = format!("{:?}", &buf);
        assert!(err.to_string().contains(&content));
    }
}
