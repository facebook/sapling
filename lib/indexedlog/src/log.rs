//! Append-only log with indexing and integrity checks
//!
//! A `Log` is logically an append-only array with one or more user-defined indexes.
//!
//! The array consists of an on-disk part and an in-memory part.
//! The on-disk part of a `Log` is stored in a managed directory, with the following files:
//!
//! - log: The plain array. The source of truth of indexes.
//! - index"i": The "i"-th index. See index.rs.
//! - index"i".sum: Checksum of the "i"-th index. See checksum_table.rs.
//! - meta: The metadata, containing the logical lengths of "log", and "index*".
//!
//! Writes to the `Log` only writes to memory, which is lock-free. Reading is always lock-free.
//! Flushing the in-memory content to disk would require a file system lock.
//!
//! Both "log" and "index*" files have checksums. So filesystem corruption will be detected.

// Detailed file formats:
//
// Primary log:
//   LOG := HEADER + ENTRY_LIST
//   HEADER := 'log\0'
//   ENTRY_LIST := '' | ENTRY_LIST + ENTRY
//   ENTRY := LEN(CONTENT) + XXHASH64(CONTENT) + CONTENT
//
// Metadata:
//   META := HEADER + XXHASH64(DATA) + LEN(DATA) + DATA
//   HEADER := 'meta\0'
//   DATA := LEN(LOG) + LEN(INDEXES) + INDEXES
//   INDEXES := '' | INDEXES + INDEX
//   INDEX := LEN(NAME) + NAME + INDEX_LOGIC_LEN
//
// Indexes:
//   See `index.rs`.
//
// Integers are VLQ encoded.

use atomicwrites::{AllowOverwrite, AtomicFile};
use index::{self, Index, InsertKey, LeafValueIter};
use memmap::Mmap;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use utils::xxhash;
use vlqencoding::{VLQDecode, VLQEncode};

/// An append-only storage with indexes and integrity checks.
pub struct Log {
    dir: PathBuf,
    disk_buf: Mmap,
    mem_buf: Vec<u8>,
    meta: LogMetadata,
    indexes: Vec<Index>,
    index_defs: Vec<IndexDef>,
}

/// Index definition.
pub struct IndexDef {
    /// How to extract index keys from an entry.
    pub func: Box<Fn(&[u8]) -> Vec<IndexOutput>>,

    /// Name of the index. Decides the file name. Change this when `func` changes.
    /// Do not abuse this by using `..` or `/`.
    pub name: &'static str,

    /// How many bytes (as counted in the primary log) could be left not indexed on-disk.
    /// The index for them would be built on-demand in-memory. This avoids some I/Os and
    /// saves some space.
    pub lag_threshold: u64,
}

/// Output of an index function - to describe a key.
pub enum IndexOutput {
    /// The index key is a reference of a range of the data, relative to the input bytes.
    Reference(Range<u64>),

    /// The index key is a separate sequence of bytes unrelated to the input bytes.
    Owned(Box<[u8]>),
}

/// Iterating through all entries in a `Log`.
pub struct LogIter<'a> {
    next_offset: u64,
    errored: bool,
    log: &'a Log,
}

/// Iterating through entries returned by an index lookup.
/// A wrapper around the index leaf value iterator.
pub struct LogLookupIter<'a> {
    inner_iter: LeafValueIter<'a>,
    errored: bool,
    log: &'a Log,
}

/// Metadata about logical file lengths.
#[derive(PartialEq, Eq, Debug)]
struct LogMetadata {
    /// Length of the primary log file.
    primary_len: u64,

    /// Lengths of index files. Name => Length.
    indexes: BTreeMap<String, u64>,
}

// Some design notes:
// - Public APIs do not expose internal offsets of entries. This avoids issues when an in-memory
//   entry gets moved after `flush`.
// - The only write-to-disk operation is `flush`, aside from creating an empty `Log`. This makes it
//   easier to verify correctness - just make sure `flush` is properly handled (ex. by locking).

impl Log {
    /// Open a log at given directory, with defined indexes. Create an empty log on demand.
    /// If `index_defs` ever changes, the caller needs to make sure the `name` is changed
    /// if `func` is changed.
    pub fn open<P: AsRef<Path>>(dir: P, index_defs: Vec<IndexDef>) -> io::Result<Self> {
        unimplemented!()
    }

    /// Append an entry in-memory. To write it on disk, use `flush`.
    pub fn append<T: AsRef<[u8]>>(&mut self, data: T) -> io::Result<()> {
        unimplemented!()
    }

    /// Write in-memory pending entries to disk.
    pub fn flush(&mut self) -> io::Result<()> {
        unimplemented!()
    }

    /// Lookup an entry using the given index. Return an iterator of `Result<&[u8]>`.
    /// `open` decides available `index_id`s.
    pub fn lookup<K: AsRef<[u8]>>(&self, index_id: usize, key: K) -> io::Result<LogLookupIter> {
        unimplemented!()
    }

    /// Return an iterator for all entries.
    pub fn iter(&self) -> LogIter {
        unimplemented!()
    }
}

impl LogMetadata {
    const HEADER: &'static [u8] = b"meta\0";

    fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
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

        Ok(Self {
            primary_len,
            indexes,
        })
    }

    fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let mut buf = Vec::new();
        buf.write_vlq(self.primary_len)?;
        buf.write_vlq(self.indexes.len())?;
        for (name, len) in self.indexes.iter() {
            let name = name.as_bytes();
            buf.write_vlq(name.len())?;
            buf.write_all(name)?;
            buf.write_vlq(*len)?;
        }
        writer.write_all(Self::HEADER)?;
        writer.write_vlq(xxhash(&buf))?;
        writer.write_vlq(buf.len())?;
        writer.write_all(&buf)?;

        Ok(())
    }

    fn read_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = fs::OpenOptions::new().read(true).open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let mut cur = Cursor::new(buf);
        Self::read(&mut cur)
    }

    fn write_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let mut buf = Vec::new();
        self.write(&mut buf)?;
        AtomicFile::new(path, AllowOverwrite).write(|f| f.write_all(&buf))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    quickcheck! {
        fn test_roundtrip_meta(primary_len: u64, indexes: BTreeMap<String, u64>) -> bool {
            let mut buf = Vec::new();
            let meta = LogMetadata { primary_len, indexes };
            meta.write(&mut buf).expect("write");
            let mut cur = Cursor::new(buf);
            let meta_read = LogMetadata::read(&mut cur).expect("read");
            meta_read == meta
        }

        fn test_roundtrip_meta_file(primary_len: u64, indexes: BTreeMap<String, u64>) -> bool {
            let dir = TempDir::new("log").expect("tempdir");
            let meta = LogMetadata { primary_len, indexes };
            let path = dir.path().join("meta");
            meta.write_file(&path).expect("write_file");
            let meta_read = LogMetadata::read_file(&path).expect("read_file");
            meta_read == meta
        }
    }
}
