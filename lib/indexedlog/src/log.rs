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
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use utils::{mmap_readonly, xxhash};
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
    // Whether the index and the log is out-of-sync. In which case, index-based reads (lookups)
    // should return errors because it can no longer be trusted.
    // This could be improved to be per index. For now, it's a single state for simplicity. It's
    // probably fine considering index corruptions are rare.
    index_corrupted: bool,
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
            index_corrupted: false,
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
        let primary_path = self.dir.join(PRIMARY_FILE);
        let mut primary_file = fs::OpenOptions::new()
            .read(true)
            .append(true)
            .open(&primary_path)?;

        let physical_len = primary_file.seek(SeekFrom::End(0))?;
        if physical_len < meta.primary_len {
            let msg = format!(
                "corrupted: {} (expected at least {} bytes)",
                primary_path.to_string_lossy(),
                meta.primary_len
            );
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, msg));
        }

        // Actually write the primary log. Once it's written, we can remove the in-memory buffer.
        primary_file.write_all(&self.mem_buf)?;
        meta.primary_len += self.mem_buf.len() as u64;

        // Step 3: Reload primary log and indexes to get the latest view.
        let (disk_buf, indexes) = Self::load_log_and_indexes(&self.dir, &meta, &self.index_defs)?;

        self.meta = meta;
        self.disk_buf = disk_buf;
        self.indexes = indexes;
        self.mem_buf.clear();

        // Step 4: Update the indexes. Optionally flush them.
        let indexes_to_flush: Vec<usize> = self.index_defs
            .iter()
            .enumerate()
            .filter(|&(_i, def)| {
                let indexed = self.meta.indexes.get(def.name).cloned().unwrap_or(0);
                indexed + def.lag_threshold < self.meta.primary_len
            })
            .map(|(i, _def)| i)
            .collect();
        self.update_indexes_for_on_disk_entries()?;
        for i in indexes_to_flush {
            let new_length = self.indexes[i].flush();
            let new_length = self.maybe_set_index_error(new_length)?;
            let name = self.index_defs[i].name.to_string();
            self.meta.indexes.insert(name, new_length);
        }

        // Step 5: Write the updated meta file.
        self.meta.write_file(self.dir.join(META_FILE))?;

        Ok(())
    }

    /// Lookup an entry using the given index. Return an iterator of `Result<&[u8]>`.
    /// `open` decides available `index_id`s.
    pub fn lookup<K: AsRef<[u8]>>(&self, index_id: usize, key: K) -> io::Result<LogLookupIter> {
        self.maybe_return_index_error()?;
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
        let result = self.update_indexes_for_in_memory_entry_unchecked(data, offset);
        self.maybe_set_index_error(result)
    }

    fn update_indexes_for_in_memory_entry_unchecked(
        &mut self,
        data: &[u8],
        offset: u64,
    ) -> io::Result<()> {
        for (mut index, def) in self.indexes.iter_mut().zip(&self.index_defs) {
            for index_output in (def.func)(data) {
                match index_output {
                    IndexOutput::Reference(range) => {
                        assert!(range.start <= range.end && range.end <= data.len() as u64);
                        // Cannot use InsertKey::Reference here since the index only has
                        // "log.disk_buf" without "log.mem_buf".
                        let key = InsertKey::Embed(&data[range.start as usize..range.end as usize]);
                        index.insert_advanced(key, offset, None)?;
                    }
                    IndexOutput::Owned(key) => {
                        let key = InsertKey::Embed(&key);
                        index.insert_advanced(key, offset, None)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Build in-memory index so they cover all entries stored in self.disk_buf.
    fn update_indexes_for_on_disk_entries(&mut self) -> io::Result<()> {
        let result = self.update_indexes_for_on_disk_entries_unchecked();
        self.maybe_set_index_error(result)
    }

    fn update_indexes_for_on_disk_entries_unchecked(&mut self) -> io::Result<()> {
        // It's a programming error to call this when mem_buf is not empty.
        assert!(self.mem_buf.is_empty());
        for (mut index, def) in self.indexes.iter_mut().zip(&self.index_defs) {
            // The index meta is used to store the next offset the index should be built.
            let mut offset = {
                let index_meta = index.get_meta();
                if index_meta.is_empty() {
                    // New index. Start processing at the first entry.
                    PRIMARY_START_OFFSET
                } else {
                    index_meta.read_vlq_at(0)?.0
                }
            };
            // PERF: might be worthwhile to cache xxhash verification result.
            while let Some(entry_result) = Self::read_entry_from_buf(&self.disk_buf, offset)? {
                let data = entry_result.data;
                for index_output in (def.func)(data) {
                    match index_output {
                        IndexOutput::Reference(range) => {
                            assert!(range.start <= range.end && range.end <= data.len() as u64);
                            let start = range.start + entry_result.data_offset;
                            let end = range.end + entry_result.data_offset;
                            let key = InsertKey::Reference((start, end - start));

                            index.insert_advanced(key, offset, None)?;
                        }
                        IndexOutput::Owned(key) => {
                            let key = InsertKey::Embed(&key);
                            index.insert_advanced(key, offset, None)?;
                        }
                    }
                }
                offset = entry_result.next_offset;
            }
            // The index now contains all entries. Write "next_offset" as the index meta.
            let mut index_meta = Vec::new();
            index_meta.write_vlq(offset)?;
            index.set_meta(index_meta);
        }
        Ok(())
    }

    /// Read `LogMetadata` from given directory. Create an empty one on demand.
    fn load_or_create_meta(dir: &Path) -> io::Result<LogMetadata> {
        match LogMetadata::read_file(dir.join(META_FILE)) {
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    // Create (and truncate) the primary log and indexes.
                    fs::create_dir_all(dir)?;
                    let mut primary_file = File::create(dir.join(PRIMARY_FILE))?;
                    primary_file.write_all(PRIMARY_HEADER)?;
                    // Start from empty file and indexes.
                    Ok(LogMetadata {
                        primary_len: PRIMARY_START_OFFSET,
                        indexes: BTreeMap::new(),
                    })
                } else {
                    Err(err)
                }
            }
            Ok(meta) => Ok(meta),
        }
    }

    /// Read (log.disk_buf, indexes) from the directory using the metadata.
    fn load_log_and_indexes(
        dir: &Path,
        meta: &LogMetadata,
        index_defs: &Vec<IndexDef>,
    ) -> io::Result<(Mmap, Vec<Index>)> {
        let primary_file = fs::OpenOptions::new()
            .read(true)
            .open(dir.join(PRIMARY_FILE))?;

        let primary_buf = mmap_readonly(&primary_file, meta.primary_len.into())?.0;
        let key_buf = Rc::new(mmap_readonly(&primary_file, meta.primary_len.into())?.0);
        let mut indexes = Vec::with_capacity(index_defs.len());
        for def in index_defs.iter() {
            let index_len = meta.indexes.get(def.name).cloned().unwrap_or(0);
            indexes.push(Self::load_index(
                dir,
                &def.name,
                index_len,
                key_buf.clone(),
            )?);
        }
        Ok((primary_buf, indexes))
    }

    /// Load a single index.
    fn load_index(dir: &Path, name: &str, len: u64, buf: Rc<AsRef<[u8]>>) -> io::Result<Index> {
        // 1MB index checksum. This makes checksum file within one block (4KB) for 512MB index.
        const INDEX_CHECKSUM_CHUNK_SIZE: u64 = 0x100000;
        let path = dir.join(format!("{}{}", INDEX_FILE_PREFIX, name));
        index::OpenOptions::new()
            .checksum_chunk_size(INDEX_CHECKSUM_CHUNK_SIZE)
            .logical_len(Some(len))
            .key_buf(Some(buf))
            .open(path)
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

    /// Wrapper around a `Result` returned by an index write operation.
    /// Make sure all index write operations are wrapped by this method.
    #[inline]
    fn maybe_set_index_error<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() && !self.index_corrupted {
            self.index_corrupted = true;
        }
        result
    }

    /// Wrapper to return an error if `index_corrupted` is set.
    /// Use this before doing index read operations.
    #[inline]
    fn maybe_return_index_error(&self) -> io::Result<()> {
        if self.index_corrupted {
            let msg = format!("index is corrupted");
            Err(io::Error::new(io::ErrorKind::InvalidData, msg))
        } else {
            Ok(())
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

impl<'a> Iterator for LogLookupIter<'a> {
    type Item = io::Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        match self.inner_iter.next() {
            None => None,
            Some(Err(err)) => {
                self.errored = true;
                Some(Err(err))
            }
            Some(Ok(offset)) => match self.log.read_entry(offset) {
                Ok(Some(entry)) => Some(Ok(entry.data)),
                Ok(None) => None,
                Err(err) => {
                    self.errored = true;
                    Some(Err(err))
                }
            },
        }
    }
}

impl<'a> LogLookupIter<'a> {
    /// A convenient way to get data.
    pub fn into_vec(self) -> io::Result<Vec<&'a [u8]>> {
        self.collect()
    }
}

impl<'a> Iterator for LogIter<'a> {
    type Item = io::Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        match self.log.read_entry(self.next_offset) {
            Err(e) => {
                self.errored = true;
                Some(Err(e))
            }
            Ok(Some(entry_result)) => {
                assert!(entry_result.next_offset > self.next_offset);
                self.next_offset = entry_result.next_offset;
                Some(Ok(entry_result.data))
            }
            Ok(None) => None,
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

    #[test]
    fn test_empty_log() {
        let dir = TempDir::new("log").unwrap();
        let log_path = dir.path().join("log");
        let log1 = Log::open(&log_path, Vec::new()).unwrap();
        assert_eq!(log1.iter().count(), 0);
        let log2 = Log::open(&log_path, Vec::new()).unwrap();
        assert_eq!(log2.iter().count(), 0);
    }

    fn get_index_defs(lag_threshold: u64) -> Vec<IndexDef> {
        // Two index functions. First takes every 2 bytes as references. The second takes every 3
        // bytes as owned slices.
        let index_func0 = |data: &[u8]| {
            (0..(data.len().max(1) - 1))
                .map(|i| IndexOutput::Reference(i as u64..i as u64 + 2))
                .collect()
        };
        let index_func1 = |data: &[u8]| {
            (0..(data.len().max(2) - 2))
                .map(|i| IndexOutput::Owned(data[i..i + 3].to_vec().into_boxed_slice()))
                .collect()
        };
        vec![
            IndexDef {
                func: Box::new(index_func0),
                name: "x",
                lag_threshold,
            },
            IndexDef {
                func: Box::new(index_func1),
                name: "y",
                lag_threshold,
            },
        ]
    }

    #[test]
    fn test_index_manual() {
        // Test index lookups with these combinations:
        // - Index key: Reference and Owned.
        // - Index lag_threshold: 0, 20, 1000.
        // - Entries: Mixed on-disk and in-memory ones.
        for lag in [0u64, 20, 1000].iter().cloned() {
            let dir = TempDir::new("log").expect("tempdir");
            let mut log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
            let entries: [&[u8]; 6] = [b"1", b"", b"2345", b"", b"78", b"3456"];
            for bytes in entries.iter() {
                log.append(bytes).expect("append");
                // Flush and reload in the middle of entries. This exercises the code paths
                // handling both on-disk and in-memory parts.
                if bytes.is_empty() {
                    log.flush().expect("flush");
                    log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
                }
            }

            // Lookups via index 0
            assert_eq!(
                log.lookup(0, b"34").unwrap().into_vec().unwrap(),
                [b"3456", b"2345"]
            );
            assert_eq!(log.lookup(0, b"56").unwrap().into_vec().unwrap(), [b"3456"]);
            assert_eq!(log.lookup(0, b"78").unwrap().into_vec().unwrap(), [b"78"]);
            assert!(log.lookup(0, b"89").unwrap().into_vec().unwrap().is_empty());

            // Lookups via index 1
            assert_eq!(
                log.lookup(1, b"345").unwrap().into_vec().unwrap(),
                [b"3456", b"2345"]
            );
        }
    }

    #[test]
    fn test_index_reorder() {
        let dir = TempDir::new("log").expect("tempdir");
        let indexes = get_index_defs(0);
        let mut log = Log::open(dir.path(), indexes).unwrap();
        let entries: [&[u8]; 2] = [b"123", b"234"];
        for bytes in entries.iter() {
            log.append(bytes).expect("append");
        }
        log.flush().expect("flush");
        // Reverse the index to make it interesting.
        let mut indexes = get_index_defs(0);
        indexes.reverse();
        log = Log::open(dir.path(), indexes).unwrap();
        assert_eq!(
            log.lookup(1, b"23").unwrap().into_vec().unwrap(),
            [b"234", b"123"]
        );
    }

    #[test]
    fn test_index_mark_corrupt() {
        let dir = TempDir::new("log").expect("tempdir");
        let indexes = get_index_defs(0);

        let mut log = Log::open(dir.path(), indexes).unwrap();
        let entries: [&[u8]; 2] = [b"123", b"234"];
        for bytes in entries.iter() {
            log.append(bytes).expect("append");
        }
        log.flush().expect("flush");

        // Corrupt an index. Backup its content.
        let backup = {
            let mut buf = Vec::new();
            let size = File::open(dir.path().join("index-x"))
                .unwrap()
                .read_to_end(&mut buf)
                .unwrap();
            let mut index_file = File::create(dir.path().join("index-x")).unwrap();
            index_file.write_all(&vec![0; size]).expect("write");
            buf
        };

        // Inserting a new entry will mark the index as "corrupted".
        assert!(log.append(b"new").is_err());

        // Then index lookups will return errors. Even if its content is restored.
        let mut index_file = File::create(dir.path().join("index-x")).unwrap();
        index_file.write_all(&backup).expect("write");
        assert!(log.lookup(1, b"23").is_err());
    }

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

        fn test_roundtrip_entries(entries: Vec<(Vec<u8>, bool, bool)>) -> bool {
            let dir = TempDir::new("log").unwrap();
            let mut log = Log::open(dir.path(), Vec::new()).unwrap();
            for &(ref data, flush, reload) in &entries {
                log.append(data).expect("append");
                if flush {
                    log.flush().expect("flush");
                    if reload {
                        log = Log::open(dir.path(), Vec::new()).unwrap();
                    }
                }
            }
            let retrived: Vec<Vec<u8>> = log.iter().map(|v| v.unwrap().to_vec()).collect();
            let entries: Vec<Vec<u8>> = entries.iter().map(|v| v.0.clone()).collect();
            retrived == entries
        }
    }
}
