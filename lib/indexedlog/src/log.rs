// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Append-only storage with indexes and integrity checks.
//!
//! See [`Log`] for the main structure. This module also provides surrounding
//! types needed to construct the [`Log`], including [`IndexDef`] and some
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

use crate::errors::{data_error, parameter_error};
use crate::index::{self, Index, InsertKey, LeafValueIter, RangeIter, ReadonlyBuffer};
use crate::lock::ScopedFileLock;
use crate::utils::{atomic_write, mmap_readonly, open_dir, xxhash, xxhash32};
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use failure::{self, Fallible};
use memmap::Mmap;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::ops::{Range, RangeBounds};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

// Constants about file names
pub(crate) const PRIMARY_FILE: &str = "log";
const PRIMARY_HEADER: &[u8] = b"indexedlog0\0";
const PRIMARY_START_OFFSET: u64 = 12; // PRIMARY_HEADER.len() as u64;
const META_FILE: &str = "meta";
const INDEX_FILE_PREFIX: &str = "index-";

const ENTRY_FLAG_HAS_XXHASH64: u32 = 1;
const ENTRY_FLAG_HAS_XXHASH32: u32 = 2;

/// An append-only storage with indexes and integrity checks.
///
/// The [`Log`] is backed by a directory in the filesystem. The
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
    disk_buf: Arc<Mmap>,
    mem_buf: Pin<Box<Vec<u8>>>,
    meta: LogMetadata,
    indexes: Vec<Index>,
    // Whether the index and the log is out-of-sync. In which case, index-based reads (lookups)
    // should return errors because it can no longer be trusted.
    // This could be improved to be per index. For now, it's a single state for simplicity. It's
    // probably fine considering index corruptions are rare.
    index_corrupted: bool,
    open_options: OpenOptions,
}

/// Definition of an index. It includes: name, function to extract index keys,
/// and how much the index can lag on disk.
#[derive(Clone)]
pub struct IndexDef {
    /// Function to extract index keys from an entry.
    ///
    /// The input is bytes of an entry (ex. the data passed to [`Log::append`]).
    /// The output is an array of index keys. An entry can have zero or more
    /// than one index keys for a same index.
    ///
    /// The output can be an allocated slice of bytes, or a reference to offsets
    /// in the input. See [`IndexOutput`] for details.
    ///
    /// The function should be pure and fast. i.e. It should not use inputs
    /// from other things, like the network, filesystem, or an external random
    /// generator.
    ///
    /// For example, if the [`Log`] is to store git commits, and the index is to
    /// help finding child commits given parent commit hashes as index keys.
    /// This function gets the commit metadata as input. It then parses the
    /// input, and extract parent commit hashes as the output. A git commit can
    /// have 0 or 1 or 2 or even more parents. Therefore the output is a [`Vec`].
    func: fn(&[u8]) -> Vec<IndexOutput>,

    /// Name of the index.
    ///
    /// The name will be used as part of the index file name. Therefore do not
    /// use user-generated content here. And do not abuse this by using `..` or `/`.
    ///
    /// When adding new or changing index functions, make sure a different
    /// `name` is used so the existing index won't be reused incorrectly.
    name: &'static str,

    /// How many bytes (as counted in the file backing [`Log`]) could be left not
    /// indexed on-disk.
    ///
    /// This is related to [`Index`] implementation detail. Since it's append-only
    /// and needs to write `O(log N)` data for updating a single entry. Allowing
    /// lagged indexes reduces writes and saves disk space.
    ///
    /// The lagged part of the index will be built on-demand in-memory by
    /// [`Log::open`].
    ///
    /// Practically, this correlates to how fast `func` is.
    lag_threshold: u64,
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
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ChecksumType {
    /// Choose xxhash64 or xxhash32 automatically based on data size.
    Auto,

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

/// Iterator over all entries in a [`Log`].
pub struct LogIter<'a> {
    next_offset: u64,
    errored: bool,
    log: &'a Log,
}

/// Iterator over [`Log`] entries selected by an index lookup.
///
/// It is a wrapper around [index::LeafValueIter].
pub struct LogLookupIter<'a> {
    inner_iter: LeafValueIter<'a>,
    errored: bool,
    log: &'a Log,
}

/// Iterator over keys and [`LogLookupIter`], filtered by an index prefix.
///
/// It is a wrapper around [index::RangeIter].
pub struct LogRangeIter<'a> {
    inner_iter: RangeIter<'a>,
    errored: bool,
    log: &'a Log,
    index: &'a Index,
}

/// Metadata about index names, logical [`Log`] and [`Index`] file lengths.
/// Used internally.
#[derive(PartialEq, Eq, Debug)]
struct LogMetadata {
    /// Length of the primary log file.
    primary_len: u64,

    /// Lengths of index files. Name => Length.
    indexes: BTreeMap<String, u64>,
}

/// Options used to configured how an [`Log`] is opened.
#[derive(Clone)]
pub struct OpenOptions {
    index_defs: Vec<IndexDef>,
    pub(crate) create: bool,
    checksum_type: ChecksumType,
    pub(crate) flush_filter: Option<FlushFilterFunc>,
}

pub(crate) type FlushFilterFunc = fn(&FlushFilterContext, &[u8]) -> Fallible<FlushFilterOutput>;

/// Potentially useful context for the flush filter function.
pub struct FlushFilterContext<'a> {
    /// The [`log`] being flushed.
    pub log: &'a Log,
}

/// Output of a flush filter.
pub enum FlushFilterOutput {
    /// Insert the entry as is.
    Keep,

    /// Remove this entry.
    Drop,

    /// Replace this entry with the specified new content.
    Replace(Vec<u8>),
}

/// Satisfy [`index::ReadonlyBuffer`] trait so [`Log`] can use external
/// keys on [`Index`] for in-memory-only entries.
struct ExternalKeyBuffer {
    disk_buf: Arc<Mmap>,
    disk_len: u64,
}

// Some design notes:
// - Public APIs do not expose internal offsets of entries. This avoids issues when an in-memory
//   entry gets moved after `flush`.
// - The only write-to-disk operation is `flush`, aside from creating an empty `Log`. This makes it
//   easier to verify correctness - just make sure `flush` is properly handled (ex. by locking).

impl Log {
    /// Construct [`Log`] at given directory. Incrementally build up specified
    /// indexes.
    ///
    /// Create the [`Log`] if it does not exist.
    ///
    /// See [`OpenOptions::open`] for details.
    pub fn open<P: AsRef<Path>>(dir: P, index_defs: Vec<IndexDef>) -> Fallible<Self> {
        OpenOptions::new()
            .index_defs(index_defs)
            .create(true)
            .open(dir)
    }

    /// Append an entry in-memory. Update related indexes in-memory.
    ///
    /// The memory part is not shared. Therefore other [`Log`] instances won't see
    /// the change immediately.
    ///
    /// To write in-memory entries and indexes to disk, call [`Log::sync`].
    pub fn append<T: AsRef<[u8]>>(&mut self, data: T) -> Fallible<()> {
        let data = data.as_ref();

        let checksum_type = if self.open_options.checksum_type == ChecksumType::Auto {
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
            if data.len() >= XXHASH64_THRESHOLD {
                ChecksumType::Xxhash64
            } else {
                ChecksumType::Xxhash32
            }
        } else {
            self.open_options.checksum_type
        };

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
            ChecksumType::Auto => unreachable!(),
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
            ChecksumType::Auto => unreachable!(),
        };

        self.mem_buf.write_all(data)?;
        self.update_indexes_for_in_memory_entry(data, offset)?;
        Ok(())
    }

    /// Load the latest data from disk. Write in-memory entries to disk.
    ///
    /// After writing, update on-disk indexes. These happen in a same critical
    /// section, protected by a lock on the directory.
    ///
    /// Even if [`Log::append`] is never called, this function has a side effect
    /// updating the [`Log`] to contain latest entries on disk.
    ///
    /// Other [`Log`] instances living in a same process or other processes won't
    /// be notified about the change and they can only access the data
    /// "snapshotted" at open time.
    ///
    /// Return the size of the updated primary log file in bytes.
    pub fn sync(&mut self) -> Fallible<u64> {
        // Read-only fast path - no need to take directory lock.
        if self.mem_buf.is_empty() {
            *self = self.open_options.clone().open(&self.dir)?;
            return Ok(self.meta.primary_len);
        }

        // Take the lock so no other `flush` runs for this directory. Then reload meta, append
        // log, then update indexes.
        let mut dir_file = open_dir(&self.dir)?;
        let _lock = ScopedFileLock::new(&mut dir_file, true)?;

        // Step 1: Reload metadata to get the latest view of the files.
        let mut meta = Self::load_meta(&self.dir, false)?;
        let changed = self.meta != meta;

        if changed && self.open_options.flush_filter.is_some() {
            let filter = self.open_options.flush_filter.unwrap();

            // Start with a clean log that does not have dirty entries.
            let mut log = self.open_options.clone().open(&self.dir)?;

            for entry in self.iter_dirty() {
                let content = entry?;
                let context = FlushFilterContext { log: &log };
                // Re-insert entries to that clean log.
                match filter(&context, content)? {
                    FlushFilterOutput::Drop => (),
                    FlushFilterOutput::Keep => log.append(content)?,
                    FlushFilterOutput::Replace(content) => log.append(content)?,
                }
            }

            // Replace "self" so we can continue flushing the updated data.
            *self = log;
        }

        // Step 2: Append to the primary log.
        let primary_path = self.dir.join(PRIMARY_FILE);
        let mut primary_file = fs::OpenOptions::new()
            .read(true)
            .append(true)
            .open(&primary_path)?;

        let physical_len = primary_file.seek(SeekFrom::End(0))?;
        if physical_len != meta.primary_len {
            let msg = format!(
                "log file {} has {} bytes, expected {} bytes",
                primary_path.to_string_lossy(),
                physical_len,
                meta.primary_len
            );
            return Err(data_error(msg));
        }

        // Actually write the primary log. Once it's written, we can remove the in-memory buffer.
        primary_file.write_all(&self.mem_buf)?;
        meta.primary_len += self.mem_buf.len() as u64;

        // Step 3: Reload primary log and indexes to get the latest view.
        let (disk_buf, indexes) =
            Self::load_log_and_indexes(&self.dir, &meta, &self.open_options.index_defs)?;

        self.meta = meta;
        self.disk_buf = disk_buf;
        self.indexes = indexes;
        self.mem_buf.clear();

        // Step 4: Update the indexes. Optionally flush them.
        let indexes_to_flush: Vec<usize> = self
            .open_options
            .index_defs
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
            let name = self.open_options.index_defs[i].name.to_string();
            self.meta.indexes.insert(name, new_length);
        }

        // Step 5: Write the updated meta file.
        self.meta.write_file(self.dir.join(META_FILE))?;

        Ok(self.meta.primary_len)
    }

    /// Renamed. Use [`Log::sync`] instead.
    pub fn flush(&mut self) -> Fallible<u64> {
        self.sync()
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` passed to [`Log::open`].
    ///
    /// Return an iterator of `Result<&[u8]>`, in reverse insertion order.
    pub fn lookup<K: AsRef<[u8]>>(&self, index_id: usize, key: K) -> Fallible<LogLookupIter> {
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
            Err(parameter_error(msg))
        }
    }

    /// Look up keys and entries using the given prefix.
    /// The `index_id` is the index of `index_defs` passed to [`Log::open`].
    ///
    /// Return an iterator that yields `(key, iter)`, where `key` is the full
    /// key, `iter` is [`LogLookupIter`] that allows iteration through matched
    /// entries.
    pub fn lookup_prefix<K: AsRef<[u8]>>(
        &self,
        index_id: usize,
        prefix: K,
    ) -> Fallible<LogRangeIter> {
        let index = self.indexes.get(index_id).unwrap();
        let inner_iter = index.scan_prefix(prefix)?;
        Ok(LogRangeIter {
            inner_iter,
            errored: false,
            log: self,
            index,
        })
    }

    /// Look up keys and entries by querying a specified index about a specified
    /// range.
    ///
    /// The `index_id` is the index of `index_defs` defined by [`OpenOptions`].
    ///
    /// Return an iterator that yields `(key, iter)`, where `key` is the full
    /// key, `iter` is [`LogLookupIter`] that allows iteration through entries
    /// matching that key.
    pub fn lookup_range<'a>(
        &self,
        index_id: usize,
        range: impl RangeBounds<&'a [u8]>,
    ) -> Fallible<LogRangeIter> {
        let index = self.indexes.get(index_id).unwrap();
        let inner_iter = index.range(range)?;
        Ok(LogRangeIter {
            inner_iter,
            errored: false,
            log: self,
            index,
        })
    }

    /// Look up keys and entries using the given hex prefix.
    /// The length of the hex string can be odd.
    ///
    /// Return an iterator that yields `(key, iter)`, where `key` is the full
    /// key, `iter` is [`LogLookupIter`] that allows iteration through matched
    /// entries.
    pub fn lookup_prefix_hex<K: AsRef<[u8]>>(
        &self,
        index_id: usize,
        hex_prefix: K,
    ) -> Fallible<LogRangeIter> {
        let index = self.indexes.get(index_id).unwrap();
        let inner_iter = index.scan_prefix_hex(hex_prefix)?;
        Ok(LogRangeIter {
            inner_iter,
            errored: false,
            log: self,
            index,
        })
    }

    /// Return an iterator for all entries.
    pub fn iter(&self) -> LogIter {
        LogIter {
            log: self,
            next_offset: PRIMARY_START_OFFSET,
            errored: false,
        }
    }

    /// Return an iterator for in-memory entries that haven't been flushed to disk.
    pub fn iter_dirty(&self) -> LogIter {
        LogIter {
            log: self,
            next_offset: self.meta.primary_len,
            errored: false,
        }
    }

    /// Applies the given index function to the entry data and returns the index keys.
    pub fn index_func<'a>(&self, index_id: usize, entry: &'a [u8]) -> Fallible<Vec<Cow<'a, [u8]>>> {
        let index_def = self.open_options.index_defs.get(index_id).ok_or_else(|| {
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
    fn update_indexes_for_in_memory_entry(&mut self, data: &[u8], offset: u64) -> Fallible<()> {
        let result = self.update_indexes_for_in_memory_entry_unchecked(data, offset);
        self.maybe_set_index_error(result)
    }

    fn update_indexes_for_in_memory_entry_unchecked(
        &mut self,
        data: &[u8],
        offset: u64,
    ) -> Fallible<()> {
        for (index, def) in self.indexes.iter_mut().zip(&self.open_options.index_defs) {
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
    fn update_indexes_for_on_disk_entries(&mut self) -> Fallible<()> {
        let result = self.update_indexes_for_on_disk_entries_unchecked();
        self.maybe_set_index_error(result)
    }

    fn update_indexes_for_on_disk_entries_unchecked(&mut self) -> Fallible<()> {
        // It's a programming error to call this when mem_buf is not empty.
        assert!(self.mem_buf.is_empty());
        for (index, def) in self.indexes.iter_mut().zip(&self.open_options.index_defs) {
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

    /// Read [`LogMetadata`] from the given directory. If `create` is `true`,
    /// create an empty one on demand.
    ///
    /// The caller should ensure the directory exists and take a lock on it to
    /// avoid filesystem races.
    fn load_meta(dir: &Path, create: bool) -> Fallible<LogMetadata> {
        let meta_path = dir.join(META_FILE);
        match LogMetadata::read_file(&meta_path) {
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound && create {
                    // Create (and truncate) the primary log and indexes.
                    let mut primary_file = File::create(dir.join(PRIMARY_FILE))?;
                    primary_file.write_all(PRIMARY_HEADER)?;
                    // Start from empty file and indexes.
                    let meta = LogMetadata {
                        primary_len: PRIMARY_START_OFFSET,
                        indexes: BTreeMap::new(),
                    };
                    meta.write_file(&meta_path)?;
                    Ok(meta)
                } else {
                    Err(data_error("cannot read meta file").context(err).into())
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
    ) -> Fallible<(Arc<Mmap>, Vec<Index>)> {
        let primary_file = fs::OpenOptions::new()
            .read(true)
            .open(dir.join(PRIMARY_FILE))?;

        let primary_buf = Arc::new(mmap_readonly(&primary_file, meta.primary_len.into())?.0);
        let key_buf = Arc::new(ExternalKeyBuffer {
            disk_buf: primary_buf.clone(),
            disk_len: meta.primary_len,
        });
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
        buf: Arc<ReadonlyBuffer + Send + Sync>,
    ) -> Fallible<Index> {
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
    fn read_entry(&self, offset: u64) -> Fallible<Option<EntryResult>> {
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
    fn read_entry_from_buf(buf: &[u8], offset: u64) -> Fallible<Option<EntryResult>> {
        if offset == buf.len() as u64 {
            return Ok(None);
        } else if offset > buf.len() as u64 {
            let msg = format!("invalid read offset {}", offset);
            return Err(data_error(msg));
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
                let checksum = LittleEndian::read_u64(
                    &buf.get(offset as usize..offset as usize + 8)
                        .ok_or_else(|| {
                            data_error(format!("xxhash cannot be read at {}", offset))
                        })?,
                );
                (checksum, offset + 8)
            }
            ENTRY_FLAG_HAS_XXHASH32 => {
                let checksum = LittleEndian::read_u32(
                    &buf.get(offset as usize..offset as usize + 4)
                        .ok_or_else(|| {
                            data_error(format!("xxhash32 cannot be read at {}", offset))
                        })?,
                ) as u64;
                (checksum, offset + 4)
            }
            _ => {
                return Err(data_error(format!(
                    "entry at {} cannot have multiple checksums",
                    offset
                )));
            }
        };

        // Read the actual payload
        let end = offset + data_len;
        if end > buf.len() as u64 {
            return Err(data_error(format!("incomplete entry data at {}", offset)));
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
            Err(data_error(format!("integrity check failed at {}", offset)))
        }
    }

    /// Wrapper around a `Result` returned by an index write operation.
    /// Make sure all index write operations are wrapped by this method.
    #[inline]
    fn maybe_set_index_error<T>(&mut self, result: Fallible<T>) -> Fallible<T> {
        if result.is_err() && !self.index_corrupted {
            self.index_corrupted = true;
        }
        result
    }

    /// Wrapper to return an error if `index_corrupted` is set.
    /// Use this before doing index read operations.
    #[inline]
    fn maybe_return_index_error(&self) -> Fallible<()> {
        if self.index_corrupted {
            let msg = format!("index is corrupted");
            Err(data_error(msg))
        } else {
            Ok(())
        }
    }
}

impl IndexDef {
    /// Create an index definition.
    ///
    /// `index_func` is the function to extract index keys from an entry.
    ///
    /// The input is bytes of an entry (ex. the data passed to [`Log::append`]).
    /// The output is an array of index keys. An entry can have zero or more
    /// than one index keys for a same index.
    ///
    /// The output can be an allocated slice of bytes, or a reference to offsets
    /// in the input. See [`IndexOutput`] for details.
    ///
    /// The function should be pure and fast. i.e. It should not use inputs
    /// from other things, like the network, filesystem, or an external random
    /// generator.
    ///
    /// For example, if the [`Log`] is to store git commits, and the index is to
    /// help finding child commits given parent commit hashes as index keys.
    /// This function gets the commit metadata as input. It then parses the
    /// input, and extract parent commit hashes as the output. A git commit can
    /// have 0 or 1 or 2 or even more parents. Therefore the output is a [`Vec`].
    ///
    /// `name` is the name of the index.
    ///
    /// The name will be used as part of the index file name. Therefore do not
    /// use user-generated content here. And do not abuse this by using `..` or `/`.
    ///
    /// When adding new or changing index functions, make sure a different
    /// `name` is used so the existing index won't be reused incorrectly.
    pub fn new(name: &'static str, index_func: fn(&[u8]) -> Vec<IndexOutput>) -> Self {
        Self {
            func: index_func,
            name,
            // For a typical commit hash index (20-byte). IndexedLog insertion
            // overhead is about 1500 entries per millisecond. Allow about 3ms
            // lagging in that case.
            lag_threshold: 5000,
        }
    }

    /// Set how many bytes (as counted in the file backing [`Log`]) could be left
    /// not indexed on-disk.
    ///
    /// This is related to [`Index`] implementation detail. Since it's append-only
    /// and needs to write `O(log N)` data for updating a single entry. Allowing
    /// lagged indexes reduces writes and saves disk space.
    ///
    /// The lagged part of the index will be built on-demand in-memory by
    /// [`Log::open`].
    ///
    /// Practically, this correlates to how fast `func` is.
    pub fn lag_threshold(self, lag_threshold: u64) -> Self {
        Self {
            func: self.func,
            name: self.name,
            lag_threshold,
        }
    }
}

impl OpenOptions {
    /// Creates a blank new set of options ready for configuration.
    ///
    /// `create` is initially `false`.
    /// `index_defs` is initially empty.
    pub fn new() -> Self {
        Self {
            create: false,
            index_defs: Vec::new(),
            checksum_type: ChecksumType::Auto,
            flush_filter: None,
        }
    }

    /// Add an index function.
    ///
    /// This is a convenient way to define indexes without using [`IndexDef`]
    /// explictly.
    pub fn index(mut self, name: &'static str, func: fn(&[u8]) -> Vec<IndexOutput>) -> Self {
        self.index_defs.push(IndexDef::new(name, func));
        self
    }

    /// Sets index definitions.
    ///
    /// See [`IndexDef::new`] for details.
    pub fn index_defs(mut self, index_defs: Vec<IndexDef>) -> Self {
        self.index_defs = index_defs;
        self
    }

    /// Sets the option for whether creating a new [`Log`] if it does not exist.
    ///
    /// If set to `true`, [`OpenOptions::open`] will create the [`Log`] on demand if
    /// it does not already exist. If set to `false`, [`OpenOptions::open`] will
    /// fail if the log does not exist.
    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    /// Sets the checksum type.
    ///
    /// See [`ChecksumType`] for details.
    pub fn checksum_type(mut self, checksum_type: ChecksumType) -> Self {
        self.checksum_type = checksum_type;
        self
    }

    /// Sets the flush filter function.
    ///
    /// The function will be called at [`Log::sync`] time, if there are
    /// changes to the `log` since `open` (or last `sync`) time.
    ///
    /// The filter function can be used to avoid writing content that already
    /// exists in the [`Log`], or rewrite content as needed.
    pub fn flush_filter(mut self, flush_filter: Option<FlushFilterFunc>) -> Self {
        self.flush_filter = flush_filter;
        self
    }

    /// Construct [`Log`] at given directory. Incrementally build up specified
    /// indexes.
    ///
    /// If the directory does not exist and `create` is set to `true`, it will
    /// be created with essential files populated. After that, an empty [`Log`]
    /// will be returned. Otherwise, `open` will fail.
    ///
    /// See [`IndexDef`] for index definitions. Indexes can be added, removed, or
    /// reordered, as long as a same `name` indicates a same index function.
    /// That is, when an index function is changed, the caller is responsible
    /// for changing the index name.
    ///
    /// Driven by the "immutable by default" idea, together with append-only
    /// properties, this structure is different from some traditional *mutable*
    /// databases backed by the filesystem:
    /// - Data are kind of "snapshotted and frozen" at open time. Mutating
    ///   files do not affect the view of instantiated [`Log`]s.
    /// - Writes are buffered until [`Log::sync`] is called.
    /// This maps to traditional "database transaction" concepts: a [`Log`] is
    /// always bounded to a transaction. [`Log::sync`] is like committing the
    /// transaction. Dropping the [`Log`] instance is like abandoning a
    /// transaction.
    pub fn open(self, dir: impl AsRef<Path>) -> Fallible<Log> {
        let dir = dir.as_ref();
        let create = self.create;

        let meta = Log::load_meta(dir, false).or_else(|_| {
            if create {
                fs::create_dir_all(dir)?;
                // Make sure check and write happens atomically.
                let mut dir_file = open_dir(dir)?;
                let _lock = ScopedFileLock::new(&mut dir_file, true)?;
                Log::load_meta(dir, true)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Log {:?} does not exist", &dir),
                )
                .into())
            }
        })?;

        let mem_buf = Box::pin(Vec::new());
        let (disk_buf, indexes) = Log::load_log_and_indexes(dir, &meta, &self.index_defs)?;
        let mut log = Log {
            dir: dir.to_path_buf(),
            disk_buf,
            mem_buf,
            meta,
            indexes,
            index_corrupted: false,
            open_options: self,
        };
        log.update_indexes_for_on_disk_entries()?;
        Ok(log)
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
    type Item = Fallible<&'a [u8]>;

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
    pub fn into_vec(self) -> Fallible<Vec<&'a [u8]>> {
        self.collect()
    }
}

impl<'a> Iterator for LogIter<'a> {
    type Item = Fallible<&'a [u8]>;

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

impl<'a> LogRangeIter<'a> {
    /// Wrap `next()` or `next_back()` result by the inner iterator.
    fn wrap_inner_next_result(
        &mut self,
        item: Option<Fallible<(Cow<'a, [u8]>, index::LinkOffset)>>,
    ) -> Option<Fallible<(Cow<'a, [u8]>, LogLookupIter<'a>)>> {
        match item {
            None => None,
            Some(Err(err)) => {
                self.errored = true;
                Some(Err(err))
            }
            Some(Ok((key, link_offset))) => {
                let iter = LogLookupIter {
                    inner_iter: link_offset.values(self.index),
                    errored: false,
                    log: self.log,
                };
                Some(Ok((key, iter)))
            }
        }
    }
}

impl<'a> Iterator for LogRangeIter<'a> {
    type Item = Fallible<(Cow<'a, [u8]>, LogLookupIter<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        let inner = self.inner_iter.next();
        self.wrap_inner_next_result(inner)
    }
}

impl<'a> DoubleEndedIterator for LogRangeIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        let inner = self.inner_iter.next_back();
        self.wrap_inner_next_result(inner)
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
        atomic_write(path, &buf)?;
        Ok(())
    }
}

impl IndexOutput {
    fn into_cow(self, data: &[u8]) -> Fallible<Cow<[u8]>> {
        Ok(match self {
            IndexOutput::Reference(range) => Cow::Borrowed(
                &data
                    .get(range.start as usize..range.end as usize)
                    .ok_or_else(|| {
                        let msg = format!("invalid range {:?} (len={})", range, data.len());
                        io::Error::new(io::ErrorKind::InvalidData, msg)
                    })?,
            ),
            IndexOutput::Owned(key) => Cow::Owned(key.into_vec()),
        })
    }
}

impl Debug for Log {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut iter = self.iter();
        loop {
            let offset = iter.next_offset;
            write!(f, "Entry[{}]: ", offset)?;
            match iter.next() {
                None => break,
                Some(Ok(bytes)) => write!(f, "{{ bytes: {:?} }}\n", bytes)?,
                Some(Err(err)) => write!(f, "{{ error: {:?} }}\n", err)?,
            }
        }
        Ok(())
    }
}

impl ReadonlyBuffer for ExternalKeyBuffer {
    #[inline]
    fn slice(&self, start: u64, len: u64) -> &[u8] {
        assert!(start < self.disk_len);
        &self.disk_buf[(start as usize)..(start + len) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_empty_log() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("log");
        let log1 = Log::open(&log_path, Vec::new()).unwrap();
        assert_eq!(log1.iter().count(), 0);
        let log2 = Log::open(&log_path, Vec::new()).unwrap();
        assert_eq!(log2.iter().count(), 0);
    }

    #[test]
    fn test_open_options_create() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("log1");

        let opts = OpenOptions::new();
        assert!(opts.open(&log_path).is_err());

        let opts = OpenOptions::new().create(true);
        assert!(opts.open(&log_path).is_ok());

        let opts = OpenOptions::new().create(false);
        assert!(opts.open(&log_path).is_ok());

        let log_path = dir.path().join("log2");
        let opts = OpenOptions::new().create(false);
        assert!(opts.open(&log_path).is_err());
    }

    #[test]
    fn test_checksum_type() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("log");

        let open = |checksum_type| {
            OpenOptions::new()
                .checksum_type(checksum_type)
                .create(true)
                .open(&log_path)
                .unwrap()
        };

        let short_bytes = vec![12; 20];
        let long_bytes = vec![24; 200];
        let mut expected = Vec::new();

        let mut log = open(ChecksumType::Auto);
        log.append(&short_bytes).unwrap();
        expected.push(short_bytes.clone());
        log.append(&long_bytes).unwrap();
        expected.push(long_bytes.clone());
        log.sync().unwrap();

        let mut log = open(ChecksumType::None);
        log.append(&short_bytes).unwrap();
        expected.push(short_bytes.clone());
        log.sync().unwrap();

        let mut log = open(ChecksumType::Xxhash32);
        log.append(&long_bytes).unwrap();
        expected.push(long_bytes.clone());
        log.sync().unwrap();

        let mut log = open(ChecksumType::Xxhash64);
        log.append(&short_bytes).unwrap();
        expected.push(short_bytes.clone());

        assert_eq!(
            log.iter()
                .map(|v| v.unwrap().to_vec())
                .collect::<Vec<Vec<u8>>>(),
            expected,
        );

        // Reload and verify
        assert_eq!(log.sync().unwrap(), 508);

        let log = Log::open(&log_path, Vec::new()).unwrap();
        assert_eq!(
            log.iter()
                .map(|v| v.unwrap().to_vec())
                .collect::<Vec<Vec<u8>>>(),
            expected,
        );
    }

    #[test]
    fn test_iter_and_iter_dirty() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("log");
        let mut log = Log::open(&log_path, Vec::new()).unwrap();

        log.append(b"2").unwrap();
        log.append(b"4").unwrap();
        log.append(b"3").unwrap();

        assert_eq!(
            log.iter().collect::<Fallible<Vec<_>>>().unwrap(),
            vec![b"2", b"4", b"3"]
        );
        assert_eq!(
            log.iter().collect::<Fallible<Vec<_>>>().unwrap(),
            log.iter_dirty().collect::<Fallible<Vec<_>>>().unwrap(),
        );

        log.sync().unwrap();

        assert!(log
            .iter_dirty()
            .collect::<Fallible<Vec<_>>>()
            .unwrap()
            .is_empty());
        assert_eq!(
            log.iter().collect::<Fallible<Vec<_>>>().unwrap(),
            vec![b"2", b"4", b"3"]
        );

        log.append(b"5").unwrap();
        log.append(b"1").unwrap();
        assert_eq!(
            log.iter_dirty().collect::<Fallible<Vec<_>>>().unwrap(),
            vec![b"5", b"1"]
        );
        assert_eq!(
            log.iter().collect::<Fallible<Vec<_>>>().unwrap(),
            vec![b"2", b"4", b"3", b"5", b"1"]
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
            IndexDef::new("x", index_func0).lag_threshold(lag_threshold),
            IndexDef::new("y", index_func1).lag_threshold(lag_threshold),
        ]
    }

    #[test]
    fn test_index_manual() {
        // Test index lookups with these combinations:
        // - Index key: Reference and Owned.
        // - Index lag_threshold: 0, 20, 1000.
        // - Entries: Mixed on-disk and in-memory ones.
        for lag in [0u64, 20, 1000].iter().cloned() {
            let dir = tempdir().unwrap();
            let mut log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
            let entries: [&[u8]; 6] = [b"1", b"", b"2345", b"", b"78", b"3456"];
            for bytes in entries.iter() {
                log.append(bytes).expect("append");
                // Flush and reload in the middle of entries. This exercises the code paths
                // handling both on-disk and in-memory parts.
                if bytes.is_empty() {
                    log.sync().expect("flush");
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
        let dir = tempdir().unwrap();
        let indexes = get_index_defs(0);
        let mut log = Log::open(dir.path(), indexes).unwrap();
        let entries: [&[u8]; 2] = [b"123", b"234"];
        for bytes in entries.iter() {
            log.append(bytes).expect("append");
        }
        log.sync().expect("flush");
        // Reverse the index to make it interesting.
        let mut indexes = get_index_defs(0);
        indexes.reverse();
        log = Log::open(dir.path(), indexes).unwrap();
        assert_eq!(
            log.lookup(1, b"23").unwrap().into_vec().unwrap(),
            [b"234", b"123"]
        );
    }

    // This test rewrites mmaped files which is unsupoorted by Windows.
    #[cfg(not(windows))]
    #[test]
    fn test_index_mark_corrupt() {
        let dir = tempdir().unwrap();
        let indexes = get_index_defs(0);

        let mut log = Log::open(dir.path(), indexes).unwrap();
        let entries: [&[u8]; 2] = [b"123", b"234"];
        for bytes in entries.iter() {
            log.append(bytes).expect("append");
        }
        log.sync().expect("flush");

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
    fn test_lookup_prefix_and_range() {
        let dir = tempdir().unwrap();
        let index_func = |data: &[u8]| vec![IndexOutput::Reference(0..(data.len() - 1) as u64)];
        let mut log = Log::open(
            dir.path(),
            vec![IndexDef::new("simple", index_func).lag_threshold(0)],
        )
        .unwrap();

        let entries = vec![&b"aaa"[..], b"bb", b"bb"];

        for entry in entries.iter() {
            log.append(entry).unwrap();
        }

        // Test lookup_prefix

        // 0x61 == b'a'. 0x6 will match both keys: "aa" and "b".
        // "aa" matches the value "aaa", "b" matches the entries ["bb", "bb"]
        let mut iter = log.lookup_prefix_hex(0, b"6").unwrap().rev();
        assert_eq!(
            iter.next()
                .unwrap()
                .unwrap()
                .1
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![b"bb", b"bb"]
        );
        assert_eq!(iter.next().unwrap().unwrap().0.as_ref(), b"aa");
        assert!(iter.next().is_none());

        let mut iter = log.lookup_prefix(0, b"b").unwrap();
        assert_eq!(iter.next().unwrap().unwrap().0.as_ref(), b"b");
        assert!(iter.next().is_none());

        // Test lookup_range
        assert_eq!(log.lookup_range(0, &b"b"[..]..).unwrap().count(), 1);
        assert_eq!(log.lookup_range(0, ..=&b"b"[..]).unwrap().count(), 2);
        assert_eq!(
            log.lookup_range(0, &b"c"[..]..=&b"d"[..]).unwrap().count(),
            0
        );

        let mut iter = log.lookup_range(0, ..).unwrap().rev();
        let next = iter.next().unwrap().unwrap();
        assert_eq!(next.0.as_ref(), &b"b"[..]);
        assert_eq!(
            next.1.collect::<Result<Vec<_>, _>>().unwrap(),
            vec![&b"bb"[..], &b"bb"[..]]
        );
        let next = iter.next().unwrap().unwrap();
        assert_eq!(next.0.as_ref(), &b"aa"[..]);
        assert_eq!(
            next.1.collect::<Result<Vec<_>, _>>().unwrap(),
            vec![&b"aaa"[..]]
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_index_func() {
        let dir = tempdir().unwrap();
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
        let third_index = |_: &[u8]| vec![IndexOutput::Owned(Box::from(&b"x"[..]))];
        let mut log = OpenOptions::new()
            .create(true)
            .index_defs(vec![
                IndexDef::new("first", first_index).lag_threshold(0),
                IndexDef::new("second", second_index).lag_threshold(0),
            ])
            .index("third", third_index)
            .open(dir.path())
            .unwrap();

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
        assert_eq!(log.iter().count(), log.lookup(2, b"x").unwrap().count());
    }

    #[test]
    fn test_flush_filter() {
        let dir = tempdir().unwrap();

        let write_by_log2 = || {
            let mut log2 = OpenOptions::new()
                .create(true)
                .flush_filter(Some(|_, _| panic!("log2 flush filter should not run")))
                .open(dir.path())
                .unwrap();
            log2.append(b"log2").unwrap();
            log2.sync().unwrap();
        };

        let mut log1 = OpenOptions::new()
            .create(true)
            .flush_filter(Some(|ctx: &FlushFilterContext, bytes: &[u8]| {
                // "new" changes by log2 are visible.
                assert_eq!(ctx.log.iter().nth(0).unwrap().unwrap(), b"log2");
                Ok(match bytes.len() {
                    1 => FlushFilterOutput::Drop,
                    2 => FlushFilterOutput::Replace(b"cc".to_vec()),
                    4 => return Err(data_error("length 4 is unsupported!")),
                    _ => FlushFilterOutput::Keep,
                })
            }))
            .open(dir.path())
            .unwrap();

        log1.append(b"a").unwrap(); // dropped
        log1.append(b"bb").unwrap(); // replaced to "cc"
        log1.append(b"ccc").unwrap(); // kept
        write_by_log2();
        log1.sync().unwrap();

        assert_eq!(
            log1.iter().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![&b"log2"[..], b"cc", b"ccc"]
        );

        log1.append(b"dddd").unwrap(); // error
        write_by_log2();
        log1.sync().unwrap_err();
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
            let dir = tempdir().unwrap();
            let meta = LogMetadata { primary_len, indexes };
            let path = dir.path().join("meta");
            meta.write_file(&path).expect("write_file");
            let meta_read = LogMetadata::read_file(&path).expect("read_file");
            meta_read == meta
        }

        fn test_roundtrip_entries(entries: Vec<(Vec<u8>, bool, bool)>) -> bool {
            let dir = tempdir().unwrap();
            let mut log = Log::open(dir.path(), Vec::new()).unwrap();
            for &(ref data, flush, reload) in &entries {
                log.append(data).expect("append");
                if flush {
                    log.sync().expect("flush");
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
