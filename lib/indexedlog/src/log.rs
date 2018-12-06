// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Append-only storage with indexes and integrity checks.
//!
//! See [Log] for the main structure. This module also provides surrounding
//! types needed to construct the [Log], including [IndexDef] and some
//! iterators.

// Detailed file formats:
//
// Primary log:
//   LOG := HEADER + ENTRY_LIST
//   HEADER := 'log\0'
//   ENTRY_LIST := '' | ENTRY_LIST + ENTRY
//   ENTRY := ENTRY_FLAGS + LEN(CONTENT) + CHECKSUM + CONTENT
//   CHECKSUM := '' | XXHASH64(CONTENT) | XXHASH32(CONTENT)
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
// Integers are VLQ encoded, except for XXHASH64 and XXHASH32, which uses
// LittleEndian encoding.

use atomicwrites::{AllowOverwrite, AtomicFile};
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use index::{self, Index, InsertKey, LeafValueIter};
use lock::ScopedFileLock;
use memmap::Mmap;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use utils::{mmap_readonly, xxhash, xxhash32};
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

// Constants about file names
const PRIMARY_FILE: &str = "log";
const PRIMARY_HEADER: &[u8] = b"indexedlog0\0";
const PRIMARY_START_OFFSET: u64 = 12; // PRIMARY_HEADER.len() as u64;
const META_FILE: &str = "meta";
const INDEX_FILE_PREFIX: &str = "index-";

const ENTRY_FLAG_HAS_XXHASH64: u32 = 1;
const ENTRY_FLAG_HAS_XXHASH32: u32 = 2;

/// An append-only storage with indexes and integrity checks.
///
/// The [Log] is backed by a directory in the filesystem. The
/// directory includes:
///
/// - An append-only "log" file. It can be seen as a serialization
///   result of an append-only list of byte slices. Each byte slice
///   has a checksum.
/// - Multiple user-defined indexes. Each index has an append-only
///   on-disk radix-tree representation and a small, separate,
///   non-append-only checksum file.
/// - A small "metadata" file which records the logic lengths (in bytes)
///   for the log and index files.
///
/// Reading is lock-free because the log and indexes are append-only.
/// Writes are buffered in memory. Flushing in-memory parts to
/// disk requires taking a flock on the directory.
pub struct Log {
    pub dir: PathBuf,
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

/// Definition of an index. It includes: name, function to extract index keys,
/// and how much the index can lag on disk.
pub struct IndexDef {
    /// Function to extract index keys from an entry.
    ///
    /// The input is bytes of an entry (ex. the data passed to [Log::append]).
    /// The output is an array of index keys. An entry can have zero or more
    /// than one index keys for a same index.
    ///
    /// The output can be an allocated slice of bytes, or a reference to offsets
    /// in the input. See [IndexOutput] for details.
    ///
    /// The function should be pure and fast. i.e. It should not use inputs
    /// from other things, like the network, filesystem, or an external random
    /// generator.
    ///
    /// For example, if the [Log] is to store git commits, and the index is to
    /// help finding child commits given parent commit hashes as index keys.
    /// This function gets the commit metadata as input. It then parses the
    /// input, and extract parent commit hashes as the output. A git commit can
    /// have 0 or 1 or 2 or even more parents. Therefore the output is a [Vec].
    pub func: Box<Fn(&[u8]) -> Vec<IndexOutput> + Send + Sync>,

    /// Name of the index.
    ///
    /// The name will be used as part of the index file name. Therefore do not
    /// use user-generated content here. And do not abuse this by using `..` or `/`.
    ///
    /// When adding new or changing index functions, make sure a different
    /// `name` is used so the existing index won't be reused incorrectly.
    pub name: &'static str,

    /// How many bytes (as counted in the file backing [Log]) could be left not
    /// indexed on-disk.
    ///
    /// This is related to [Index] implementation detail. Since it's append-only
    /// and needs to write `O(log N)` data for updating a single entry. Allowing
    /// lagged indexes reduces writes and saves disk space.
    ///
    /// The lagged part of the index will be built on-demand in-memory by
    /// [Log::open].
    ///
    /// Practically, this correlates to how fast `func` is.
    pub lag_threshold: u64,
}

/// Output of an index function. Bytes that can be used for lookups.
pub enum IndexOutput {
    /// The index key is a slice, relative to the data entry (ex. input of the
    /// index function).
    ///
    /// Use this if possible. It generates smaller indexes.
    Reference(Range<u64>),

    /// The index key is a separate sequence of bytes unrelated to the input
    /// bytes.
    ///
    /// Use this if the index key is not in the entry. For example, if the entry
    /// is compressed.
    Owned(Box<[u8]>),
}

/// What checksum function to use for an entry.
#[derive(Copy, Clone, Debug)]
pub enum ChecksumType {
    /// No checksum. Suitable for data that have their own checksum logic.
    /// For example, source control commit data might have SHA1 that can
    /// verify themselves.
    None,

    /// Use xxhash64 checksum algorithm. Efficient on 64bit platforms.
    Xxhash64,

    /// Use xxhash64 checksum algorithm. It is slower than xxhash64 for 64bit
    /// platforms, but takes less space. Perhaps a good fit when entries are
    /// short.
    Xxhash32,
}

/// Iterator over all entries in a [Log].
pub struct LogIter<'a> {
    next_offset: u64,
    errored: bool,
    log: &'a Log,
}

/// Iterator over [Log] entries selected by an index lookup.
///
/// It is a wrapper around [index::LeafValueIter].
pub struct LogLookupIter<'a> {
    inner_iter: LeafValueIter<'a>,
    errored: bool,
    log: &'a Log,
}

/// Metadata about index names, logical [Log] and [Index] file lengths.
/// Used internally.
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
    /// Construct [Log] at given directory. Incrementally build up specified
    /// indexes.
    ///
    /// If the directory does not exist, it will be created with essential files
    /// populated. After that, an empty [Log] will be returned.
    ///
    /// See [IndexDef] for index definitions. Indexes can be added, removed, or
    /// reordered, as long as a same `name` indicates a same index function.
    /// That is, when an index function is changed, the caller is responsible
    /// for changing the index name.
    ///
    /// Driven by the "immutable by default" idea, together with append-only
    /// properties, this structure is different from some traditional *mutable*
    /// databases backed by the filesystem:
    /// - Data are kind of "snapshotted and frozen" at open time. Mutating
    ///   files do not affect the view of instantiated [Log]s.
    /// - Writes are buffered until [Log::flush] is called.
    /// This maps to traditional "database transaction" concepts: a [Log] is
    /// always bounded to a transaction. [Log::flush] is like committing the
    /// transaction. Dropping the [Log] instance is like abandoning a
    /// transaction.
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

    /// Append an entry in-memory. Update related indexes in-memory.
    ///
    /// The memory part is not shared. Therefore other [Log] instances won't see
    /// the change immediately.
    ///
    /// To write in-memory entries and indexes to disk, call [Log::flush].
    pub fn append<T: AsRef<[u8]>>(&mut self, data: T) -> io::Result<()> {
        // xxhash64 is slower for smaller data. A quick benchmark on x64 platform shows:
        //
        // bytes  xxhash32  xxhash64 (MB/s)
        //   32       1882      1600
        //   40       1739      1538
        //   48       2285      1846
        //   56       2153      2000
        //   64       2666      2782
        //   72       2400      2322
        //   80       2962      2758
        //   88       2750      2750
        //   96       3200      3692
        //  104       2810      3058
        //  112       3393      3500
        //  120       3000      3428
        //  128       3459      4266
        const XXHASH64_THRESHOLD: usize = 88;
        let data = data.as_ref();
        let checksum_type = if data.len() >= XXHASH64_THRESHOLD {
            ChecksumType::Xxhash64
        } else {
            ChecksumType::Xxhash32
        };
        self.append_advanced(data, checksum_type)
    }

    /// Advanced version of [Log::append], with more controls, like specifying
    /// the checksum algorithm.
    pub fn append_advanced<T: AsRef<[u8]>>(
        &mut self,
        data: T,
        checksum_type: ChecksumType,
    ) -> io::Result<()> {
        let data = data.as_ref();
        let offset = self.meta.primary_len + self.mem_buf.len() as u64;

        // Design note: Currently checksum_type is the only thing that decides
        // entry_flags.  Entry flags is not designed to just cover different
        // checksum types.  For example, if we'd like to introduce transparent
        // compression (maybe not a good idea since it can be more cleanly built
        // at an upper layer), or some other ways to store data (ex. reference
        // to other data, or fixed length data), they can probably be done by
        // extending the entry type.
        let mut entry_flags = 0;
        entry_flags |= match checksum_type {
            ChecksumType::None => 0,
            ChecksumType::Xxhash64 => ENTRY_FLAG_HAS_XXHASH64,
            ChecksumType::Xxhash32 => ENTRY_FLAG_HAS_XXHASH32,
        };

        self.mem_buf.write_vlq(entry_flags)?;
        self.mem_buf.write_vlq(data.len())?;

        match checksum_type {
            ChecksumType::None => (),
            ChecksumType::Xxhash64 => {
                self.mem_buf.write_u64::<LittleEndian>(xxhash(data))?;
            }
            ChecksumType::Xxhash32 => {
                self.mem_buf.write_u32::<LittleEndian>(xxhash32(data))?;
            }
        };

        self.mem_buf.write_all(data)?;
        self.update_indexes_for_in_memory_entry(data, offset)?;
        Ok(())
    }

    /// Write in-memory entries to disk.
    ///
    /// Load the latest data from disk. Write in-memory entries to disk. Then
    /// update on-disk indexes. These happen in a same critical section,
    /// protected by a lock on the directory.
    ///
    /// Even if [Log::append] is never called, this function has a side effect
    /// updating the [Log] to contain latest entries on disk.
    ///
    /// Other [Log] instances living in a same process or other processes won't
    /// be notified about the change and they can only access the data
    /// "snapshotted" at open time.
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

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` passed to [Log::open].
    ///
    /// Return an iterator of `Result<&[u8]>`, in reverse insertion order.
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

    /// Applies the given index function to the entry data and returns the index keys.
    pub fn index_func<'a>(
        &self,
        index_id: usize,
        entry: &'a [u8],
    ) -> io::Result<Vec<Cow<'a, [u8]>>> {
        let index_def = self.index_defs.get(index_id).ok_or_else(|| {
            let msg = format!("invalid index_id {} (len={})", index_id, self.indexes.len());
            io::Error::new(io::ErrorKind::InvalidData, msg)
        })?;
        let mut result = vec![];
        for output in (index_def.func)(entry).into_iter() {
            result.push(output.into_cow(&entry)?);
        }

        Ok(result)
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

    /// Build in-memory index so they cover all entries stored in `self.disk_buf`.
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

    /// Read `(log.disk_buf, indexes)` from the directory using the metadata.
    fn load_log_and_indexes(
        dir: &Path,
        meta: &LogMetadata,
        index_defs: &Vec<IndexDef>,
    ) -> io::Result<(Mmap, Vec<Index>)> {
        let primary_file = fs::OpenOptions::new()
            .read(true)
            .open(dir.join(PRIMARY_FILE))?;

        let primary_buf = mmap_readonly(&primary_file, meta.primary_len.into())?.0;
        let key_buf = Arc::new(mmap_readonly(&primary_file, meta.primary_len.into())?.0);
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
    fn load_index(
        dir: &Path,
        name: &str,
        len: u64,
        buf: Arc<AsRef<[u8]> + Send + Sync>,
    ) -> io::Result<Index> {
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
        } else if offset > buf.len() as u64 {
            let msg = format!("invalid read offset {}", offset);
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
        }

        let (entry_flags, vlq_len): (u32, _) = buf.read_vlq_at(offset as usize)?;
        let offset = offset + vlq_len as u64;

        // For now, data_len is the next field regardless of entry flags.
        let (data_len, vlq_len): (u64, _) = buf.read_vlq_at(offset as usize)?;
        let offset = offset + vlq_len as u64;

        // Depends on entry_flags, some of them have a checksum field.
        let checksum_flags = entry_flags & (ENTRY_FLAG_HAS_XXHASH64 | ENTRY_FLAG_HAS_XXHASH32);
        let (checksum, offset) = match checksum_flags {
            0 => (0, offset),
            ENTRY_FLAG_HAS_XXHASH64 => {
                let checksum = LittleEndian::read_u64(&buf.get(
                    offset as usize..offset as usize + 8,
                ).ok_or_else(|| invalid(format!("xxhash cannot be read at {}", offset)))?);
                (checksum, offset + 8)
            }
            ENTRY_FLAG_HAS_XXHASH32 => {
                let checksum = LittleEndian::read_u32(&buf.get(
                    offset as usize..offset as usize + 4,
                ).ok_or_else(|| invalid(format!("xxhash32 cannot be read at {}", offset)))?)
                    as u64;
                (checksum, offset + 4)
            }
            _ => {
                return Err(invalid(format!(
                    "entry at {} cannot have multiple checksums",
                    offset
                )))
            }
        };

        // Read the actual payload
        let end = offset + data_len;
        if end > buf.len() as u64 {
            return Err(invalid(format!("incomplete entry data at {}", offset)));
        }
        let data = &buf[offset as usize..end as usize];

        let verified = match checksum_flags {
            0 => true,
            ENTRY_FLAG_HAS_XXHASH64 => xxhash(&data) == checksum,
            ENTRY_FLAG_HAS_XXHASH32 => xxhash32(&data) as u64 == checksum,
            // Tested above. Therefore unreachable.
            _ => unreachable!(),
        };
        if verified {
            Ok(Some(EntryResult {
                data,
                data_offset: offset,
                next_offset: end,
            }))
        } else {
            Err(invalid(format!("integrity check failed at {}", offset)))
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

/// "Pointer" to an entry. Used internally.
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

impl IndexOutput {
    fn into_cow(self, data: &[u8]) -> io::Result<Cow<[u8]>> {
        Ok(match self {
            IndexOutput::Reference(range) => Cow::Borrowed(&data.get(
                range.start as usize..range.end as usize,
            ).ok_or_else(|| {
                let msg = format!("invalid range {:?} (len={})", range, data.len());
                io::Error::new(io::ErrorKind::InvalidData, msg)
            })?),
            IndexOutput::Owned(key) => Cow::Owned(key.into_vec()),
        })
    }
}

// Shorter way to construct an "InvalidData" error.
fn invalid(message: String) -> io::Error {
    return io::Error::new(io::ErrorKind::InvalidData, message);
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

    #[test]
    fn test_append_advanced() {
        let dir = TempDir::new("log").unwrap();
        let log_path = dir.path().join("log");
        let mut log = Log::open(&log_path, Vec::new()).unwrap();

        let short_bytes = vec![12; 20];
        let long_bytes = vec![24; 200];
        let mut expected = Vec::new();

        log.append(&short_bytes).unwrap();
        expected.push(short_bytes.clone());
        log.append(&long_bytes).unwrap();
        expected.push(long_bytes.clone());
        log.append_advanced(&short_bytes, ChecksumType::None)
            .unwrap();
        expected.push(short_bytes.clone());
        log.append_advanced(&long_bytes, ChecksumType::Xxhash32)
            .unwrap();
        expected.push(long_bytes.clone());
        log.append_advanced(&short_bytes, ChecksumType::Xxhash64)
            .unwrap();
        expected.push(short_bytes.clone());

        assert_eq!(
            log.iter()
                .map(|v| v.unwrap().to_vec())
                .collect::<Vec<Vec<u8>>>(),
            expected,
        );

        // Reload and verify
        log.flush().unwrap();
        let log = Log::open(&log_path, Vec::new()).unwrap();
        assert_eq!(
            log.iter()
                .map(|v| v.unwrap().to_vec())
                .collect::<Vec<Vec<u8>>>(),
            expected,
        );
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

    #[test]
    fn test_index_func() {
        let dir = TempDir::new("log").unwrap();
        let entries = vec![
            b"abcdefghij",
            b"klmnopqrst",
            b"uvwxyz1234",
            b"5678901234",
            b"5678901234",
        ];

        let first_index =
            |_data: &[u8]| vec![IndexOutput::Reference(0..2), IndexOutput::Reference(3..5)];
        let second_index = |data: &[u8]| vec![IndexOutput::Owned(Box::from(&data[5..10]))];
        let mut log = Log::open(
            dir.path(),
            vec![
                IndexDef {
                    func: Box::new(first_index),
                    name: "first",
                    lag_threshold: 0,
                },
                IndexDef {
                    func: Box::new(second_index),
                    name: "second",
                    lag_threshold: 0,
                },
            ],
        ).unwrap();

        let mut expected_keys1 = vec![];
        let mut expected_keys2 = vec![];
        for &data in entries {
            log.append(data).expect("append");
            expected_keys1.push(data[0..2].to_vec());
            expected_keys1.push(data[3..5].to_vec());
            expected_keys2.push(data[5..10].to_vec());
        }

        let mut found_keys1 = vec![];
        let mut found_keys2 = vec![];

        for entry in log.iter() {
            let entry = entry.unwrap();
            found_keys1.extend(
                log.index_func(0, &entry)
                    .unwrap()
                    .into_iter()
                    .map(|c| c.into_owned()),
            );
            found_keys2.extend(
                log.index_func(1, &entry)
                    .unwrap()
                    .into_iter()
                    .map(|c| c.into_owned()),
            );
        }
        assert_eq!(found_keys1, expected_keys1);
        assert_eq!(found_keys2, expected_keys2);
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
