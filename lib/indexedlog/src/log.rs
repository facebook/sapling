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
use lock::ScopedFileLock;
use memmap::Mmap;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use utils::xxhash;
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

// Constants about file names
const PRIMARY_FILE: &str = "log";
const PRIMARY_HEADER: &[u8] = b"indexedlog0\0";
const PRIMARY_START_OFFSET: u64 = 12; // PRIMARY_HEADER.len() as u64;
const META_FILE: &str = "meta";
const INDEX_FILE_PREFIX: &str = "index-";

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
        let dir = dir.as_ref();
        let meta = Self::load_or_create_meta(dir)?;
        let (disk_buf, indexes) = Self::load_log_and_indexes(dir, &meta, &index_defs)?;
        let mut log = Log {
            dir: dir.to_path_buf(),
            disk_buf,
            mem_buf: Vec::new(),
            meta,
            indexes,
            index_defs,
        };
        log.update_indexes_for_on_disk_entries()?;
        Ok(log)
    }

    /// Append an entry in-memory. To write it on disk, use `flush`.
    pub fn append<T: AsRef<[u8]>>(&mut self, data: T) -> io::Result<()> {
        let data = data.as_ref();
        let offset = self.meta.primary_len + self.mem_buf.len() as u64;
        // ENTRY := LEN(CONTENT) + XXHASH64(CONTENT) + CONTENT
        self.mem_buf.write_vlq(data.len())?;
        self.mem_buf.write_vlq(xxhash(data))?;
        self.mem_buf.write_all(data)?;
        self.update_indexes_for_in_memory_entry(data, offset)?;
        Ok(())
    }

    /// Write in-memory pending entries to disk.
    pub fn flush(&mut self) -> io::Result<()> {
        // Take the lock so no other `flush` runs for this directory. Then reload meta, append
        // log, then update indexes.
        let mut dir_file = fs::OpenOptions::new().read(true).open(&self.dir)?;
        let _lock = ScopedFileLock::new(&mut dir_file, true)?;

        // Step 1: Reload metadata to get the latest view of the files.
        let mut meta = Self::load_or_create_meta(&self.dir)?;

        // Step 2: Append to the primary log.
        unimplemented!();

        // Step 3: Reload primary log and indexes to get the latest view.
        let (disk_buf, indexes) = Self::load_log_and_indexes(&self.dir, &meta, &self.index_defs)?;
        self.disk_buf = disk_buf;
        self.indexes = indexes;

        // Step 4: Update the indexes. Optionally flush them.
        self.update_indexes_for_on_disk_entries()?;

        // Step 5: Write the updated meta file.
        self.meta.write_file(self.dir.join(META_FILE))?;

        Ok(())
    }

    /// Lookup an entry using the given index. Return an iterator of `Result<&[u8]>`.
    /// `open` decides available `index_id`s.
    pub fn lookup<K: AsRef<[u8]>>(&self, index_id: usize, key: K) -> io::Result<LogLookupIter> {
        if let Some(index) = self.indexes.get(index_id) {
            assert!(key.as_ref().len() > 0);
            let link_offset = index.get(&key)?;
            let inner_iter = link_offset.values(index);
            Ok(LogLookupIter {
                inner_iter,
                errored: false,
                log: self,
            })
        } else {
            let msg = format!("invalid index_id {} (len={})", index_id, self.indexes.len());
            Err(io::Error::new(io::ErrorKind::InvalidData, msg))
        }
    }

    /// Return an iterator for all entries.
    pub fn iter(&self) -> LogIter {
        LogIter {
            log: self,
            next_offset: PRIMARY_START_OFFSET,
            errored: false,
        }
    }

    /// Build in-memory index for the newly added entry.
    fn update_indexes_for_in_memory_entry(&mut self, data: &[u8], offset: u64) -> io::Result<()> {
        unimplemented!()
    }

    /// Build in-memory index so they cover all entries stored in self.disk_buf.
    fn update_indexes_for_on_disk_entries(&mut self) -> io::Result<()> {
        unimplemented!()
    }

    /// Read `LogMetadata` from given directory. Create an empty one on demand.
    fn load_or_create_meta(dir: &Path) -> io::Result<LogMetadata> {
        unimplemented!()
    }

    /// Read (log.disk_buf, indexes) from the directory using the metadata.
    fn load_log_and_indexes(
        dir: &Path,
        meta: &LogMetadata,
        index_defs: &Vec<IndexDef>,
    ) -> io::Result<(Mmap, Vec<Index>)> {
        unimplemented!()
    }

    /// Read the entry at the given offset. Return `None` if offset is out of bound, or the content
    /// of the data, the real offset of the data, and the next offset. Raise errors if
    /// integrity-check failed.
    fn read_entry(&self, offset: u64) -> io::Result<Option<EntryResult>> {
        let result = if offset < self.meta.primary_len {
            Self::read_entry_from_buf(&self.disk_buf, offset)?
        } else {
            let offset = offset - self.meta.primary_len;
            if offset >= self.mem_buf.len() as u64 {
                return Ok(None);
            }
            Self::read_entry_from_buf(&self.mem_buf, offset)?
                .map(|entry_result| entry_result.offset(self.meta.primary_len))
        };
        Ok(result)
    }

    /// Read an entry at the given offset of the given buffer. Verify its integrity. Return the
    /// data, the real data offset, and the next entry offset. Return None if the offset is at
    /// the end of the buffer.  Raise errors if there are integrity check issues.
    fn read_entry_from_buf(buf: &[u8], offset: u64) -> io::Result<Option<EntryResult>> {
        if offset == buf.len() as u64 {
            return Ok(None);
        }
        let (data_len, vlq_len): (u64, _) = buf.read_vlq_at(offset as usize)?;
        let offset = offset + vlq_len as u64;
        let (checksum, vlq_len) = buf.read_vlq_at(offset as usize)?;
        let offset = offset + vlq_len as u64;
        let end = offset + data_len;
        if end > buf.len() as u64 {
            let msg = format!("entry data out of range");
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, msg));
        }
        let data = &buf[offset as usize..end as usize];
        if xxhash(&data) != checksum {
            let msg = format!("integrity check failed at {}", offset);
            Err(io::Error::new(io::ErrorKind::InvalidData, msg))
        } else {
            Ok(Some(EntryResult {
                data,
                data_offset: offset,
                next_offset: end,
            }))
        }
    }
}

// Entry data used internally.
struct EntryResult<'a> {
    data: &'a [u8],
    data_offset: u64,
    next_offset: u64,
}

impl<'a> EntryResult<'a> {
    /// Add some value to `next_offset`.
    fn offset(self, offset: u64) -> EntryResult<'a> {
        EntryResult {
            data: self.data,
            // `data_offset` is relative to the current buffer (disk_buf, or mem_buf).
            // So it does not need to be changed.
            data_offset: self.data_offset,
            next_offset: self.next_offset + offset,
        }
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
