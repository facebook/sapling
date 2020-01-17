/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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

use crate::checksum_table::ChecksumTable;
use crate::errors::{IoResultExt, ResultExt};
use crate::index::{self, Index, InsertKey, InsertValue, LeafValueIter, RangeIter, ReadonlyBuffer};
use crate::lock::ScopedDirLock;
use crate::repair::OpenOptionsRepair;
use crate::utils::{self, atomic_write, mmap_empty, mmap_len, xxhash, xxhash32};
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use memmap::Mmap;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::fs::{self, File};
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::ops::{Range, RangeBounds};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tracing::debug_span;
use vlqencoding::{VLQDecodeAt, VLQEncode};

mod meta;
pub use self::meta::LogMetadata;

// Constants about file names
pub(crate) const PRIMARY_FILE: &str = "log";
const PRIMARY_HEADER: &[u8] = b"indexedlog0\0";
const PRIMARY_START_OFFSET: u64 = 12; // PRIMARY_HEADER.len() as u64;
pub(crate) const META_FILE: &str = "meta";
const INDEX_FILE_PREFIX: &str = "index-";

const ENTRY_FLAG_HAS_XXHASH64: u32 = 1;
const ENTRY_FLAG_HAS_XXHASH32: u32 = 2;

// 1MB index checksum. This makes checksum file within one block (4KB) for 512MB index.
const INDEX_CHECKSUM_CHUNK_SIZE_LOG: u32 = 20;
const INDEX_CHECKSUM_CHUNK_SIZE: u64 = 1u64 << INDEX_CHECKSUM_CHUNK_SIZE_LOG;

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
    pub dir: Option<PathBuf>,
    disk_buf: Arc<Mmap>,
    pub(crate) mem_buf: Pin<Box<Vec<u8>>>,
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

    /// Remove all values associated with the key in the index.
    ///
    /// This only affects the index. The entry is not removed in the log.
    Remove(Box<[u8]>),

    /// Remove all values associated with all keys with the given prefix in the index.
    ///
    /// This only affects the index. The entry is not removed in the log.
    RemovePrefix(Box<[u8]>),
}

/// What checksum function to use for an entry.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ChecksumType {
    /// Choose xxhash64 or xxhash32 automatically based on data size.
    Auto,

    /// Use xxhash64 checksum algorithm. Efficient on 64bit platforms.
    Xxhash64,

    /// Use xxhash32 checksum algorithm. It is slower than xxhash64 for 64bit
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

/// Options used to configured how an [`Log`] is opened.
#[derive(Clone)]
pub struct OpenOptions {
    index_defs: Vec<IndexDef>,
    pub(crate) create: bool,
    checksum_type: ChecksumType,
    pub(crate) flush_filter: Option<FlushFilterFunc>,
    fsync: bool,
    auto_sync_threshold: Option<u64>,
}

pub(crate) type FlushFilterFunc =
    fn(
        &FlushFilterContext,
        &[u8],
    ) -> Result<FlushFilterOutput, Box<dyn std::error::Error + Send + Sync + 'static>>;

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

    // Prove the pointer is valid:
    // 1. If ExternalKeyBuffer is alive, then the Index owning it is alive.
    //    This is because ExternalKeyBuffer is private to Index, and there
    //    is no way to get a clone of ExternalKeyBuffer without also
    //    cloning its owner (Index).
    // 2. If the Index owning ExternalKeyBuffer is alive, then the Log
    //    owning the Index is alive. Similarily, Index is private to Log,
    //    and there is no way to just clone the Index without cloning
    //    its owner (Log).
    // 3. If Log is alive, then Log.mem_buf is alive.
    // 4. Log.mem_buf is pinned, so this pointer is valid.
    //
    // Here is why `Arc<Mutex<Vec<u8>>>` is not fesiable:
    //
    // - Bad performance: The Mutex overhead is visible.
    //   "log insertion (no checksum)" takes 2x time.
    //   "log insertion" and "log iteration (memory)" take 1.5x time.
    // - Unsafe Rust is still necessary.
    //   In [`Log::read_entry`], reading the in-memory entry case,
    //   the borrow relationship changes from `&Log -> &[u8]` to
    //   `&Log -> &MutexGuard -> &[u8]`, which means unsafe Rust is
    //   needed, or it has to take the mutex lock. Neither desirable.
    //
    // Here is why normal liftime is not fesiable:
    // - A normal lifetime will enforce the `mem_buf` to be read-only.
    //   But Log needs to write to it.
    //
    // Note: Rust reference cannot be used here, because a reference implies
    // LLVM "noalias", which is not true since Log can change Log.mem_buf.
    //
    // (UNSAFE NOTICE)
    mem_buf: *const Vec<u8>,
}

// mem_buf can be read from multiple threads at the same time if no thread is
// changing the actual mem_buf. If there is a thread changing mem_buf by
// calling Log::append(&mut self, ...), then the compiler should make sure
// Log methods taking &self are not called at the same time.
unsafe impl Send for ExternalKeyBuffer {}
unsafe impl Sync for ExternalKeyBuffer {}

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
    pub fn open<P: AsRef<Path>>(dir: P, index_defs: Vec<IndexDef>) -> crate::Result<Self> {
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
    pub fn append<T: AsRef<[u8]>>(&mut self, data: T) -> crate::Result<()> {
        let result: crate::Result<_> = (|| {
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
                ChecksumType::Xxhash64 => ENTRY_FLAG_HAS_XXHASH64,
                ChecksumType::Xxhash32 => ENTRY_FLAG_HAS_XXHASH32,
                ChecksumType::Auto => unreachable!(),
            };

            self.mem_buf.write_vlq(entry_flags).infallible()?;
            self.mem_buf.write_vlq(data.len()).infallible()?;

            match checksum_type {
                ChecksumType::Xxhash64 => {
                    self.mem_buf
                        .write_u64::<LittleEndian>(xxhash(data))
                        .infallible()?;
                }
                ChecksumType::Xxhash32 => {
                    self.mem_buf
                        .write_u32::<LittleEndian>(xxhash32(data))
                        .infallible()?;
                }
                ChecksumType::Auto => unreachable!(),
            };
            let data_offset = self.meta.primary_len + self.mem_buf.len() as u64;

            self.mem_buf.write_all(data).infallible()?;
            self.update_indexes_for_in_memory_entry(data, offset, data_offset)?;

            if let Some(threshold) = self.open_options.auto_sync_threshold {
                if self.mem_buf.len() as u64 >= threshold {
                    self.sync()
                        .context("sync triggered by auto_sync_threshold")?;
                }
            }

            Ok(())
        })();

        result
            .context(|| {
                let data = data.as_ref();
                if data.len() < 128 {
                    format!("in Log::append({:?})", data)
                } else {
                    format!("in Log::append(<a {}-byte long slice>)", data.len())
                }
            })
            .context(|| format!("  Log.dir = {:?}", self.dir))
    }

    /// Remove dirty (in-memory) state. Restore the [`Log`] to the state as
    /// if it's just loaded from disk without modifications.
    pub fn clear_dirty(&mut self) -> crate::Result<()> {
        let result: crate::Result<_> = (|| {
            self.maybe_return_index_error()?;
            for index in self.indexes.iter_mut() {
                index.clear_dirty();
            }
            self.mem_buf.clear();
            self.update_indexes_for_on_disk_entries()?;
            Ok(())
        })();
        result
            .context("in Log::clear_dirty")
            .context(|| format!("  Log.dir = {:?}", self.dir))
    }

    /// Return a cloned [`Log`] with pending in-memory changes.
    pub fn try_clone(&self) -> crate::Result<Self> {
        self.try_clone_internal(true)
            .context("in Log:try_clone")
            .context(|| format!("  Log.dir = {:?}", self.dir))
    }

    /// Return a cloned [`Log`] without pending in-memory changes.
    ///
    /// This is logically equivalent to calling `clear_dirty` immediately
    /// on the result after `try_clone`, but potentially cheaper.
    pub fn try_clone_without_dirty(&self) -> crate::Result<Self> {
        self.try_clone_internal(false)
            .context("in Log:try_clone_without_dirty")
    }

    fn try_clone_internal(&self, copy_dirty: bool) -> crate::Result<Self> {
        self.maybe_return_index_error()?;

        // Prepare cloned versions of things.
        let mut indexes = self
            .indexes
            .iter()
            .map(|i| i.try_clone_internal(copy_dirty))
            .collect::<Result<Vec<Index>, _>>()?;
        let disk_buf = self.disk_buf.clone();
        let mem_buf = if copy_dirty {
            self.mem_buf.clone()
        } else {
            let mem_buf = Box::pin(Vec::new());
            {
                // Update external key buffer of indexes to point to the new mem_buf.
                let mem_buf: &Vec<u8> = &mem_buf;
                let mem_buf: *const Vec<u8> = mem_buf as *const Vec<u8>;
                let index_key_buf = Arc::new(ExternalKeyBuffer {
                    disk_buf: disk_buf.clone(),
                    disk_len: self.meta.primary_len,
                    mem_buf,
                });
                for index in indexes.iter_mut() {
                    index.key_buf = index_key_buf.clone();
                }
            }
            mem_buf
        };

        // Create the new Log.
        let mut log = Log {
            dir: self.dir.clone(),
            disk_buf,
            mem_buf,
            meta: self.meta.clone(),
            indexes,
            index_corrupted: false,
            open_options: self.open_options.clone(),
        };

        if !copy_dirty {
            // The indexes can be lagging. Update them.
            // This is similar to what clear_dirty does.
            log.update_indexes_for_on_disk_entries()?;
        }

        Ok(log)
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
    ///
    /// For in-memory-only Logs, this function does nothing, and returns 0.
    pub fn sync(&mut self) -> crate::Result<u64> {
        let result: crate::Result<_> = (|| {
            let span = debug_span!("Log::sync", dirty_bytes = self.mem_buf.len());
            if let Some(dir) = &self.dir {
                span.record("dir", &dir.to_string_lossy().as_ref());
            }
            let _guard = span.enter();

            if self.dir.is_none() {
                // See Index::flush for why this is not an Err.
                return Ok(0);
            }

            fn check_append_only(this: &Log, new_meta: &LogMetadata) -> crate::Result<()> {
                let old_meta = &this.meta;
                if old_meta.primary_len > new_meta.primary_len {
                    Err(crate::Error::path(this.dir.as_ref().unwrap(), format!(
                    "on-disk log is unexpectedly smaller ({} bytes) than its previous version ({} bytes)",
                    new_meta.primary_len, old_meta.primary_len
                )))
                } else {
                    Ok(())
                }
            }

            // Read-only fast path - no need to take directory lock.
            if self.mem_buf.is_empty() {
                if let Ok(meta) = Self::load_or_create_meta(&self.dir.as_ref().unwrap(), false) {
                    let changed = self.meta != meta;
                    let truncated = self.meta.epoch != meta.epoch;
                    if !truncated {
                        check_append_only(self, &meta)?;
                    }
                    // No need to reload anything if metadata hasn't changed.
                    if changed {
                        // Indexes cannot be reused, if epoch has changed. Otherwise,
                        // Indexes can be reused, since they do not have new in-memory
                        // entries, and the on-disk primary log is append-only (so data
                        // already present in the indexes is valid).
                        *self = self.open_options.clone().open_internal(
                            self.dir.as_ref().unwrap(),
                            if truncated { None } else { Some(&self.indexes) },
                            None,
                        )?;
                    }
                } else {
                    // If meta can not be read, do not error out.
                    // This Log can still be used to answer queries.
                    //
                    // This behavior makes Log more friendly for cases where an
                    // external process does a `rm -rf` and the current process
                    // does a `sync()` just for loading new data. Not erroring
                    // out and pretend that nothing happended.
                }
                return Ok(self.meta.primary_len);
            }

            // Take the lock so no other `flush` runs for this directory. Then reload meta, append
            // log, then update indexes.
            let dir = self.dir.clone().unwrap();
            let lock = ScopedDirLock::new(&dir)?;

            // Step 1: Reload metadata to get the latest view of the files.
            let mut meta = Self::load_or_create_meta(&self.dir.as_ref().unwrap(), false)?;
            let changed = self.meta != meta;
            let truncated = self.meta.epoch != meta.epoch;
            if !truncated {
                check_append_only(self, &meta)?;
            }

            // Cases where Log and Indexes need to be reloaded.
            if changed && self.open_options.flush_filter.is_some() {
                let filter = self.open_options.flush_filter.unwrap();

                // Start with a clean log that does not have dirty entries.
                let mut log = self
                    .open_options
                    .clone()
                    .open_with_lock(self.dir.as_ref().unwrap(), &lock)
                    .context("re-open to run flush_filter")?;

                for entry in self.iter_dirty() {
                    let content = entry?;
                    let context = FlushFilterContext { log: &log };
                    // Re-insert entries to that clean log.
                    match filter(&context, content)
                        .map_err(|err| crate::Error::wrap(err, "failed to run filter function"))?
                    {
                        FlushFilterOutput::Drop => (),
                        FlushFilterOutput::Keep => log.append(content)?,
                        FlushFilterOutput::Replace(content) => log.append(content)?,
                    }
                }

                // Replace "self" so we can continue flushing the updated data.
                *self = log;
            } else if truncated {
                // Reload log and indexes, and re-insert entries.
                let mut log = self
                    .open_options
                    .clone()
                    .open_with_lock(self.dir.as_ref().unwrap(), &lock)
                    .context(|| {
                        format!(
                            "re-open since epoch has changed ({} to {})",
                            self.meta.epoch, meta.epoch
                        )
                    })?;

                for entry in self.iter_dirty() {
                    let content = entry?;
                    log.append(content)?;
                }

                // Replace "self" so we can continue flushing the updated data.
                *self = log;
            }

            // Step 2: Append to the primary log.
            let primary_path = self.dir.as_ref().unwrap().join(PRIMARY_FILE);
            let mut primary_file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&primary_path)
                .context(&primary_path, "cannot open for read-write")?;

            // It's possible that the previous write was interrupted. In that case,
            // the length of "log" can be longer than the length of "log" stored in
            // the metadata. Seek to the length specified by the metadata and
            // overwrite (broken) data.
            // This breaks the "append-only" property of the physical file. But all
            // readers use "meta" to decide the length of "log". So "log" is still
            // append-only as seen by readers, as long as the length specified in
            // "meta" is append-only (i.e. "meta" is not rewritten to have a smaller
            // length, and all bytes in the specified length are immutable).
            // Note: file.set_len might easily fail on Windows due to mmap.
            let pos = primary_file
                .seek(SeekFrom::Start(meta.primary_len))
                .context(&primary_path, || {
                    format!("cannot seek to {}", meta.primary_len)
                })?;
            if pos != meta.primary_len {
                let msg = format!(
                    "log file {} has {} bytes, expect at least {} bytes",
                    primary_path.to_string_lossy(),
                    pos,
                    meta.primary_len
                );
                // This might be another process re-creating the file.
                // Do not consider this as a corruption (?).
                // TODO: Review this decision.
                let err = crate::Error::path(&primary_path, msg);
                return Err(err);
            }

            // Actually write the primary log. Once it's written, we can remove the in-memory buffer.
            primary_file
                .write_all(&self.mem_buf)
                .context(&primary_path, || {
                    format!("cannot write data ({} bytes)", self.mem_buf.len())
                })?;

            if self.open_options.fsync {
                primary_file
                    .sync_all()
                    .context(&primary_path, "cannot fsync")?;
            }

            meta.primary_len += self.mem_buf.len() as u64;
            self.mem_buf.clear();

            // Decide what indexes need to be updated on disk.
            let indexes_to_flush: Vec<usize> = self
                .open_options
                .index_defs
                .iter()
                .enumerate()
                .filter(|&(_i, def)| {
                    let indexed = self.meta.indexes.get(def.name).cloned().unwrap_or(0);
                    indexed.saturating_add(def.lag_threshold) < meta.primary_len
                })
                .map(|(i, _def)| i)
                .collect();

            // Step 3: Reload primary log and indexes to get the latest view.
            let (disk_buf, indexes) = Self::load_log_and_indexes(
                Some(self.dir.as_ref().unwrap()),
                &meta,
                &self.open_options.index_defs,
                &self.mem_buf,
                if changed {
                    // Existing indexes cannot be reused.
                    None
                } else {
                    // Indexes can be reused, because they already contain all entries
                    // that were just written to disk and the on-disk files do not
                    // have new entries (tested by "self.meta != meta" in Step 1).
                    //
                    // The indexes contain all entries, because they were previously
                    // "always-up-to-date", and the on-disk log does not have anything new.
                    // Update "meta" so "update_indexes_for_on_disk_entries" below won't
                    // re-index entries.
                    Self::set_index_log_len(self.indexes.iter_mut(), meta.primary_len);
                    Some(&self.indexes)
                },
                self.open_options.fsync,
            )?;

            self.disk_buf = disk_buf;
            self.indexes = indexes;
            self.meta = meta;

            // Step 4: Update the indexes. Optionally flush them.
            self.update_indexes_for_on_disk_entries()?;
            for i in indexes_to_flush {
                let new_length = self.indexes[i].flush();
                let new_length = self.maybe_set_index_error(new_length.map_err(Into::into))?;
                let name = self.open_options.index_defs[i].name.to_string();
                self.meta.indexes.insert(name, new_length);
            }

            // Step 5: Write the updated meta file.
            let meta_path = self.dir.as_ref().unwrap().join(META_FILE);
            self.meta.write_file(&meta_path, self.open_options.fsync)?;

            Ok(self.meta.primary_len)
        })();

        result
            .context("in Log::sync")
            .context(|| format!("  Log.dir = {:?}", self.dir))
    }

    /// Check if the log is changed on disk.
    pub fn is_changed(&self) -> bool {
        match &self.dir {
            None => false,
            Some(dir) => {
                let meta_path = dir.join(META_FILE);
                match LogMetadata::read_file(&meta_path) {
                    Ok(meta) => meta != self.meta,
                    Err(_) => true,
                }
            }
        }
    }

    /// Renamed. Use [`Log::sync`] instead.
    pub fn flush(&mut self) -> crate::Result<u64> {
        self.sync()
    }

    /// Make sure on-disk indexes are up-to-date with the primary log, regardless
    /// of `lag_threshold`.
    ///
    /// This is used internally by [`RotateLog`] to make sure a [`Log`] has
    /// complate indexes before rotating.
    pub(crate) fn finalize_indexes(&mut self) -> crate::Result<()> {
        let result: crate::Result<_> = (|| {
            if let Some(ref dir) = self.dir {
                let dir = dir.clone();
                if !self.mem_buf.is_empty() {
                    return Err(crate::Error::programming(
                        "sync() should be called before finalize_indexes()",
                    ));
                }

                let _lock = ScopedDirLock::new(&dir)?;

                let meta = Self::load_or_create_meta(&dir, false)?;
                if self.meta != meta {
                    return Err(crate::Error::programming(
                        "race detected, callsite responsible for preventing races",
                    ));
                }

                // Flush all indexes.
                for i in 0..self.indexes.len() {
                    let new_length = self.indexes[i].flush();
                    let new_length = self.maybe_set_index_error(new_length.map_err(Into::into))?;
                    let name = self.open_options.index_defs[i].name.to_string();
                    self.meta.indexes.insert(name, new_length);
                }

                let meta_path = dir.join(META_FILE);
                self.meta.write_file(&meta_path, self.open_options.fsync)?;
            }
            Ok(())
        })();
        result
            .context("in Log::finalize_indexes")
            .context(|| format!("  Log.dir = {:?}", self.dir))
    }

    /// Rebuild indexes.
    ///
    /// If `force` is `false`, then indexes that pass the checksum check
    /// will not be rebuilt. Otherwise, they will be rebuilt regardless.
    ///
    /// Setting `force` to `true` might reduce the size used by the index
    /// files. But that is more expensive.
    ///
    /// The function consumes the [`Log`] object, since it is hard to recover
    /// from an error case.
    ///
    /// Return message useful for human consumption.
    pub fn rebuild_indexes(self, force: bool) -> crate::Result<String> {
        let dir = self.dir.clone();
        let result: crate::Result<_> = (|this: Log| {
            if let Some(dir) = this.dir.clone() {
                let lock = ScopedDirLock::new(&dir)?;
                this.rebuild_indexes_with_lock(force, &lock)
            } else {
                Ok(String::new())
            }
        })(self);

        result
            .context(|| format!("in Log::rebuild_indexes(force={})", force))
            .context(|| format!("  Log.dir = {:?}", dir))
    }

    fn rebuild_indexes_with_lock(
        mut self,
        force: bool,
        _lock: &ScopedDirLock,
    ) -> crate::Result<String> {
        let mut message = String::new();
        {
            if let Some(ref dir) = self.dir {
                for (i, def) in self.open_options.index_defs.iter().enumerate() {
                    let name = def.name;

                    if let Some(index) = &self.indexes.get(i) {
                        let should_skip = if force {
                            false
                        } else {
                            match Self::get_index_log_len(index) {
                                Err(_) => false,
                                Ok(len) => {
                                    if len > self.meta.primary_len {
                                        message += &format!(
                                            "Index {:?} is incompatible with (truncated) log\n",
                                            name
                                        );
                                        false
                                    } else if index.verify().is_ok() {
                                        message +=
                                            &format!("Index {:?} passed integrity check\n", name);
                                        true
                                    } else {
                                        message +=
                                            &format!("Index {:?} failed integrity check\n", name);
                                        false
                                    }
                                }
                            }
                        };
                        if should_skip {
                            continue;
                        } else {
                            // Replace the index with a dummy, empty one.
                            //
                            // This will munmap index files, which is required on
                            // Windows to rewrite the index files. It's also the reason
                            // why it's hard to recover from an error state.
                            //
                            // This is also why this function consumes the Log object.
                            self.indexes[i] = index::OpenOptions::new().create_in_memory()?;
                        }
                    }

                    let tmp = tempfile::NamedTempFile::new_in(dir).context(&dir, || {
                        format!("cannot create tempfile for rebuilding index {:?}", name)
                    })?;
                    let index_len = {
                        let mut index = index::OpenOptions::new()
                            .key_buf(Some(self.disk_buf.clone()))
                            .open(&tmp.path())?;
                        Self::update_index_for_on_disk_entry_unchecked(
                            &self.dir,
                            &mut index,
                            def,
                            &self.disk_buf,
                            self.meta.primary_len,
                        )?;
                        index.flush()?
                    };

                    // Before replacing the index, set its "logic length" to 0 so
                    // readers won't get inconsistent view about index length and data.
                    let meta_path = dir.join(META_FILE);
                    self.meta.indexes.insert(name.to_string(), 0);
                    self.meta
                        .write_file(&meta_path, self.open_options.fsync)
                        .context(|| format!("  before replacing index {:?})", name))?;

                    let _ = utils::fix_perm_file(tmp.as_file(), false);

                    let path = dir.join(format!("{}{}", INDEX_FILE_PREFIX, name));
                    tmp.persist(&path).map_err(|e| {
                        crate::Error::wrap(Box::new(e), || {
                            format!("cannot persist tempfile to replace index {:?}", name)
                        })
                    })?;

                    // At this point, other processes might see an updated index
                    // with an outdated checksum table. That's okay because the
                    // index metadata says index len is 0. That disables checksum
                    // check.

                    // Update checksum table.
                    let mut table = ChecksumTable::new_empty(&path)?;
                    table
                        .update(Some(INDEX_CHECKSUM_CHUNK_SIZE_LOG))
                        .context("while trying to update checksum for rebuilt index")?;

                    self.meta.indexes.insert(name.to_string(), index_len);
                    self.meta
                        .write_file(&meta_path, self.open_options.fsync)
                        .context(|| format!("  after replacing index {:?}", name))?;
                    message += &format!("Rebuilt index {:?}\n", name);
                }
            }
        }

        Ok(message)
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` passed to [`Log::open`].
    ///
    /// Return an iterator of `Result<&[u8]>`, in reverse insertion order.
    pub fn lookup<K: AsRef<[u8]>>(&self, index_id: usize, key: K) -> crate::Result<LogLookupIter> {
        let result: crate::Result<_> = (|| {
            self.maybe_return_index_error()?;
            if let Some(index) = self.indexes.get(index_id) {
                assert!(!key.as_ref().is_empty());
                let link_offset = index.get(&key)?;
                let inner_iter = link_offset.values(index);
                Ok(LogLookupIter {
                    inner_iter,
                    errored: false,
                    log: self,
                })
            } else {
                let msg = format!(
                    "invalid index_id {} (len={}, path={:?})",
                    index_id,
                    self.indexes.len(),
                    &self.dir
                );
                Err(crate::Error::programming(msg))
            }
        })();
        result
            .context(|| format!("in Log::lookup({}, {:?})", index_id, key.as_ref()))
            .context(|| format!("  Log.dir = {:?}", self.dir))
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
    ) -> crate::Result<LogRangeIter> {
        let prefix = prefix.as_ref();
        let result: crate::Result<_> = (|| {
            let index = self.indexes.get(index_id).unwrap();
            let inner_iter = index.scan_prefix(prefix)?;
            Ok(LogRangeIter {
                inner_iter,
                errored: false,
                log: self,
                index,
            })
        })();
        result
            .context(|| format!("in Log::lookup_prefix({}, {:?})", index_id, prefix))
            .context(|| format!("  Log.dir = {:?}", self.dir))
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
    ) -> crate::Result<LogRangeIter> {
        let start = range.start_bound();
        let end = range.end_bound();
        let result: crate::Result<_> = (|| {
            let index = self.indexes.get(index_id).unwrap();
            let inner_iter = index.range((start, end))?;
            Ok(LogRangeIter {
                inner_iter,
                errored: false,
                log: self,
                index,
            })
        })();
        result
            .context(|| {
                format!(
                    "in Log::lookup_range({}, {:?} to {:?})",
                    index_id, start, end,
                )
            })
            .context(|| format!("  Log.dir = {:?}", self.dir))
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
    ) -> crate::Result<LogRangeIter> {
        let prefix = hex_prefix.as_ref();
        let result: crate::Result<_> = (|| {
            let index = self.indexes.get(index_id).unwrap();
            let inner_iter = index.scan_prefix_hex(prefix)?;
            Ok(LogRangeIter {
                inner_iter,
                errored: false,
                log: self,
                index,
            })
        })();
        result
            .context(|| format!("in Log::lookup_prefix_hex({}, {:?})", index_id, prefix))
            .context(|| format!("  Log.dir = {:?}", self.dir))
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
    ///
    /// For in-memory Logs, this is the same as [`Log::iter`].
    pub fn iter_dirty(&self) -> LogIter {
        LogIter {
            log: self,
            next_offset: self.meta.primary_len,
            errored: false,
        }
    }

    /// Applies the given index function to the entry data and returns the index keys.
    pub fn index_func<'a>(
        &self,
        index_id: usize,
        entry: &'a [u8],
    ) -> crate::Result<Vec<Cow<'a, [u8]>>> {
        let index_def = self.get_index_def(index_id)?;
        let mut result = vec![];
        for output in (index_def.func)(entry).into_iter() {
            result.push(
                output
                    .into_cow(&entry)
                    .context(|| format!("index_id = {}", index_id))?,
            );
        }

        Ok(result)
    }

    /// Build in-memory index for the newly added entry.
    ///
    /// `offset` is the logical start offset of the entry.
    /// `data_offset` is the logical start offset of the real data (skips
    /// length, and checksum header in the entry).
    fn update_indexes_for_in_memory_entry(
        &mut self,
        data: &[u8],
        offset: u64,
        data_offset: u64,
    ) -> crate::Result<()> {
        let result = self.update_indexes_for_in_memory_entry_unchecked(data, offset, data_offset);
        self.maybe_set_index_error(result)
    }

    fn update_indexes_for_in_memory_entry_unchecked(
        &mut self,
        data: &[u8],
        offset: u64,
        data_offset: u64,
    ) -> crate::Result<()> {
        for (index, def) in self.indexes.iter_mut().zip(&self.open_options.index_defs) {
            for index_output in (def.func)(data) {
                match index_output {
                    IndexOutput::Reference(range) => {
                        assert!(range.start <= range.end && range.end <= data.len() as u64);
                        let start = range.start + data_offset;
                        let end = range.end + data_offset;
                        let key = InsertKey::Reference((start, end - start));
                        index.insert_advanced(key, InsertValue::Prepend(offset))?;
                    }
                    IndexOutput::Owned(key) => {
                        let key = InsertKey::Embed(&key);
                        index.insert_advanced(key, InsertValue::Prepend(offset))?;
                    }
                    IndexOutput::Remove(key) => {
                        index.remove(key)?;
                    }
                    IndexOutput::RemovePrefix(key) => {
                        index.remove_prefix(key)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Build in-memory index so they cover all entries stored in `self.disk_buf`.
    fn update_indexes_for_on_disk_entries(&mut self) -> crate::Result<()> {
        let result = self.update_indexes_for_on_disk_entries_unchecked();
        self.maybe_set_index_error(result)
    }

    fn update_indexes_for_on_disk_entries_unchecked(&mut self) -> crate::Result<()> {
        // It's a programming error to call this when mem_buf is not empty.
        assert!(self.mem_buf.is_empty());
        for (index, def) in self.indexes.iter_mut().zip(&self.open_options.index_defs) {
            Self::update_index_for_on_disk_entry_unchecked(
                &self.dir,
                index,
                def,
                &self.disk_buf,
                self.meta.primary_len,
            )?;
        }
        Ok(())
    }

    fn update_index_for_on_disk_entry_unchecked(
        path: &Option<PathBuf>,
        index: &mut Index,
        def: &IndexDef,
        disk_buf: &Mmap,
        primary_len: u64,
    ) -> crate::Result<()> {
        // The index meta is used to store the next offset the index should be built.
        let mut offset = Self::get_index_log_len(index)?;
        // PERF: might be worthwhile to cache xxhash verification result.
        while let Some(entry_result) =
            Self::read_entry_from_buf(&path, disk_buf, offset).context(|| {
                format!(
                    "while updating index {:?} for on-disk entry at {}",
                    def.name, offset
                )
            })?
        {
            let data = entry_result.data;
            for index_output in (def.func)(data) {
                match index_output {
                    IndexOutput::Reference(range) => {
                        assert!(range.start <= range.end && range.end <= data.len() as u64);
                        let start = range.start + entry_result.data_offset;
                        let end = range.end + entry_result.data_offset;
                        let key = InsertKey::Reference((start, end - start));

                        index.insert_advanced(key, InsertValue::Prepend(offset))?;
                    }
                    IndexOutput::Owned(key) => {
                        let key = InsertKey::Embed(&key);
                        index.insert_advanced(key, InsertValue::Prepend(offset))?;
                    }
                    IndexOutput::Remove(key) => {
                        index.remove(key)?;
                    }
                    IndexOutput::RemovePrefix(key) => {
                        index.remove_prefix(key)?;
                    }
                }
            }
            offset = entry_result.next_offset;
        }
        // The index now contains all entries. Write "next_offset" as the index meta.
        Self::set_index_log_len(std::iter::once(index), primary_len);

        Ok(())
    }

    /// Read [`LogMetadata`] from the given directory. If `create` is `true`,
    /// create an empty one on demand.
    ///
    /// The caller should ensure the directory exists and take a lock on it to
    /// avoid filesystem races.
    fn load_or_create_meta(dir: &Path, create: bool) -> crate::Result<LogMetadata> {
        let meta_path = dir.join(META_FILE);
        match LogMetadata::read_file(&meta_path) {
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound && create {
                    // Create (and truncate) the primary log and indexes.
                    let primary_path = dir.join(PRIMARY_FILE);
                    let mut primary_file =
                        File::create(&primary_path).context(&primary_path, "cannot create")?;
                    primary_file
                        .write_all(PRIMARY_HEADER)
                        .context(&primary_path, "cannot write")?;
                    let _ = utils::fix_perm_file(&primary_file, false);
                    // Start from empty file and indexes.
                    let meta = LogMetadata {
                        primary_len: PRIMARY_START_OFFSET,
                        indexes: BTreeMap::new(),
                        epoch: utils::epoch(),
                    };
                    // An empty meta file is easy to recreate. No need to use fsync.
                    meta.write_file(&meta_path, false)?;
                    Ok(meta)
                } else {
                    Err(err).context(&meta_path, "cannot read")
                }
            }
            Ok(meta) => Ok(meta),
        }
    }

    /// Read `(log.disk_buf, indexes)` from the directory using the metadata.
    ///
    /// If `reuse_indexes` is not None, they are existing indexes that match `index_defs`
    /// order. This should only be used in `sync` code path when the on-disk `meta` matches
    /// the in-memory `meta`. Otherwise it is not a sound use.
    fn load_log_and_indexes(
        dir: Option<&Path>,
        meta: &LogMetadata,
        index_defs: &[IndexDef],
        mem_buf: &Pin<Box<Vec<u8>>>,
        reuse_indexes: Option<&Vec<Index>>,
        fsync: bool,
    ) -> crate::Result<(Arc<Mmap>, Vec<Index>)> {
        let primary_buf = match dir {
            Some(dir) => Arc::new(mmap_len(&dir.join(PRIMARY_FILE), meta.primary_len)?),
            None => Arc::new(mmap_empty().infallible()?),
        };

        let mem_buf: &Vec<u8> = &mem_buf;
        let mem_buf: *const Vec<u8> = mem_buf as *const Vec<u8>;
        let key_buf = Arc::new(ExternalKeyBuffer {
            disk_buf: primary_buf.clone(),
            disk_len: meta.primary_len,
            mem_buf,
        });

        let indexes = match reuse_indexes {
            None => {
                // No indexes are reused, reload them.
                let mut indexes = Vec::with_capacity(index_defs.len());
                for def in index_defs.iter() {
                    let index_len = meta.indexes.get(def.name).cloned().unwrap_or(0);
                    indexes.push(Self::load_index(
                        dir,
                        &def.name,
                        index_len,
                        key_buf.clone(),
                        fsync,
                    )?);
                }
                indexes
            }
            Some(indexes) => {
                assert_eq!(index_defs.len(), indexes.len());
                let mut new_indexes = Vec::with_capacity(indexes.len());
                // Avoid reloading the index from disk.
                // Update their ExternalKeyBuffer so they have the updated meta.primary_len.
                for (index, def) in indexes.iter().zip(index_defs) {
                    let index_len = meta.indexes.get(def.name).cloned().unwrap_or(0);
                    let index = if index_len > Self::get_index_log_len(index).unwrap_or(0) {
                        // The on-disk index covers more entries. Loading it is probably
                        // better than reusing the existing in-memory index.
                        Self::load_index(dir, &def.name, index_len, key_buf.clone(), fsync)?
                    } else {
                        let mut index = index.try_clone()?;
                        index.key_buf = key_buf.clone();
                        index
                    };
                    new_indexes.push(index);
                }
                new_indexes
            }
        };
        Ok((primary_buf, indexes))
    }

    /// Load a single index.
    fn load_index(
        dir: Option<&Path>,
        name: &str,
        len: u64,
        buf: Arc<dyn ReadonlyBuffer + Send + Sync>,
        fsync: bool,
    ) -> crate::Result<Index> {
        match dir {
            Some(dir) => {
                let path = dir.join(format!("{}{}", INDEX_FILE_PREFIX, name));
                index::OpenOptions::new()
                    .checksum_chunk_size(INDEX_CHECKSUM_CHUNK_SIZE)
                    .logical_len(Some(len))
                    .key_buf(Some(buf))
                    .fsync(fsync)
                    .open(path)
            }
            None => index::OpenOptions::new()
                .logical_len(Some(len))
                .key_buf(Some(buf))
                .fsync(fsync)
                .create_in_memory(),
        }
    }

    /// Read the entry at the given offset. Return `None` if offset is out of bound, or the content
    /// of the data, the real offset of the data, and the next offset. Raise errors if
    /// integrity-check failed.
    fn read_entry(&self, offset: u64) -> crate::Result<Option<EntryResult>> {
        let result = if offset < self.meta.primary_len {
            Self::read_entry_from_buf(&self.dir, &self.disk_buf, offset)?
        } else {
            let offset = offset - self.meta.primary_len;
            if offset >= self.mem_buf.len() as u64 {
                return Ok(None);
            }
            Self::read_entry_from_buf(&self.dir, &self.mem_buf, offset)?
                .map(|entry_result| entry_result.offset(self.meta.primary_len))
        };
        Ok(result)
    }

    /// Read an entry at the given offset of the given buffer. Verify its integrity. Return the
    /// data, the real data offset, and the next entry offset. Return None if the offset is at
    /// the end of the buffer.  Raise errors if there are integrity check issues.
    fn read_entry_from_buf<'a>(
        path: &Option<PathBuf>,
        buf: &'a [u8],
        offset: u64,
    ) -> crate::Result<Option<EntryResult<'a>>> {
        let data_error = |msg: String| -> crate::Error {
            match path {
                Some(path) => crate::Error::corruption(path, msg),
                None => crate::Error::path(Path::new("<memory>"), msg),
            }
        };

        use std::cmp::Ordering::{Equal, Greater};
        match offset.cmp(&(buf.len() as u64)) {
            Equal => return Ok(None),
            Greater => {
                let msg = format!("read offset {} exceeds buffer size {}", offset, buf.len());
                return Err(data_error(msg));
            }
            _ => (),
        }

        let (entry_flags, vlq_len): (u32, _) = buf.read_vlq_at(offset as usize).map_err(|e| {
            crate::Error::wrap(Box::new(e), || {
                format!("cannot read entry_flags at {}", offset)
            })
            .mark_corruption()
        })?;
        let offset = offset + vlq_len as u64;

        // For now, data_len is the next field regardless of entry flags.
        let (data_len, vlq_len): (u64, _) = buf.read_vlq_at(offset as usize).map_err(|e| {
            crate::Error::wrap(Box::new(e), || {
                format!("cannot read data_len at {}", offset)
            })
            .mark_corruption()
        })?;
        let offset = offset + vlq_len as u64;

        // Depends on entry_flags, some of them have a checksum field.
        let checksum_flags = entry_flags & (ENTRY_FLAG_HAS_XXHASH64 | ENTRY_FLAG_HAS_XXHASH32);
        let (checksum, offset) = match checksum_flags {
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
                    "entry at {} has malformed checksum metadata",
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
    fn maybe_set_index_error<T>(&mut self, result: crate::Result<T>) -> crate::Result<T> {
        if result.is_err() && !self.index_corrupted {
            self.index_corrupted = true;
        }
        result
    }

    /// Wrapper to return an error if `index_corrupted` is set.
    /// Use this before doing index read operations.
    #[inline]
    fn maybe_return_index_error(&self) -> crate::Result<()> {
        if self.index_corrupted {
            let msg = "index is corrupted".to_string();
            Err(self.corruption(msg))
        } else {
            Ok(())
        }
    }

    /// Get the log length (in bytes) covered by the given index.
    ///
    /// This only makes sense at open() or sync() time, since the data won't be updated
    /// by append() for performance reasons.
    fn get_index_log_len(index: &Index) -> crate::Result<u64> {
        let index_meta = index.get_meta();
        Ok(if index_meta.is_empty() {
            // New index. Start processing at the first entry.
            PRIMARY_START_OFFSET
        } else {
            index_meta
                .read_vlq_at(0)
                .context(&index.path, || {
                    format!(
                        "index metadata cannot be parsed as an integer: {:?}",
                        &index_meta
                    )
                })?
                .0
        })
    }

    /// Update the log length (in bytes) covered by the given indexes.
    ///
    /// `len` is usually `meta.primary_len`.
    fn set_index_log_len<'a>(indexes: impl Iterator<Item = &'a mut Index>, len: u64) {
        let mut index_meta = Vec::new();
        index_meta.write_vlq(len).unwrap();
        for index in indexes {
            index.set_meta(&index_meta);
        }
    }
}

// Error-related utilities

impl Log {
    /// Get the specified index, with error handling.
    fn get_index(&self, index_id: usize) -> crate::Result<&Index> {
        self.indexes.get(index_id).ok_or_else(|| {
            let msg = format!(
                "index_id {} is out of bound (len={}, dir={:?})",
                index_id,
                self.indexes.len(),
                &self.dir
            );
            crate::Error::programming(msg)
        })
    }

    /// Get the specified index, with error handling.
    fn get_index_def(&self, index_id: usize) -> crate::Result<&IndexDef> {
        self.open_options.index_defs.get(index_id).ok_or_else(|| {
            let msg = format!(
                "index_id {} is out of bound (len={}, dir={:?})",
                index_id,
                self.indexes.len(),
                &self.dir
            );
            crate::Error::programming(msg)
        })
    }

    fn corruption(&self, message: String) -> crate::Error {
        let path: &Path = match self.dir {
            Some(ref path) => &path,
            None => Path::new("<memory>"),
        };
        crate::Error::corruption(path, message)
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
    #[allow(clippy::new_without_default)]
    /// Creates a blank new set of options ready for configuration.
    ///
    /// `create` is initially `false`.
    /// `fsync` is initially `false`.
    /// `index_defs` is initially empty.
    /// `auto_sync_threshold` is initially `None`.
    pub fn new() -> Self {
        Self {
            create: false,
            index_defs: Vec::new(),
            checksum_type: ChecksumType::Auto,
            flush_filter: None,
            fsync: false,
            auto_sync_threshold: None,
        }
    }

    /// Set fsync behavior.
    ///
    /// If true, then [`Log::sync`] will use `fsync` to flush log and index
    /// data to the physical device before returning.
    pub fn fsync(mut self, fsync: bool) -> Self {
        self.fsync = fsync;
        self
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

    /// Sets whether to call [`Log::sync`] automatically when the in-memory
    /// buffer exceeds some size threshold.
    /// - `None`: Do not call `sync` automatically.
    /// - `Some(size)`: Call `sync` when the in-memory buffer exceeds `size`.
    /// - `Some(0)`: Call `sync` after every `append` automatically.
    pub fn auto_sync_threshold(mut self, threshold: impl Into<Option<u64>>) -> Self {
        self.auto_sync_threshold = threshold.into();
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
    pub fn open(&self, dir: impl AsRef<Path>) -> crate::Result<Log> {
        let dir = dir.as_ref();
        let span = debug_span!("Log::open", dir = &dir.to_string_lossy().as_ref());
        let _guard = span.enter();
        self.open_internal(dir, None, None)
            .context(|| format!("in log::OpenOptions::open({:?})", dir))
    }

    /// Construct an empty in-memory [`Log`] without side-effects on the
    /// filesystem. The in-memory [`Log`] cannot be [`sync`]ed.
    pub fn create_in_memory(&self) -> crate::Result<Log> {
        let result: crate::Result<_> = (|| {
            let meta = LogMetadata {
                primary_len: PRIMARY_START_OFFSET,
                indexes: BTreeMap::new(),
                epoch: utils::epoch(),
            };
            let mem_buf = Box::pin(Vec::new());
            let (disk_buf, indexes) = Log::load_log_and_indexes(
                None,
                &meta,
                &self.index_defs,
                &mem_buf,
                None,
                self.fsync,
            )?;

            Ok(Log {
                dir: None,
                disk_buf,
                mem_buf,
                meta,
                indexes,
                index_corrupted: false,
                open_options: self.clone(),
            })
        })();

        result.context("in log::OpenOptions::create_in_memory")
    }

    fn open_with_lock(&self, dir: &Path, lock: &ScopedDirLock) -> crate::Result<Log> {
        self.open_internal(dir, None, Some(lock))
    }

    // "Back-door" version of "open" that allows reusing indexes.
    // Used by [`Log::sync`]. See [`Log::load_log_and_indexes`] for when indexes
    // can be reused.
    fn open_internal(
        &self,
        dir: &Path,
        reuse_indexes: Option<&Vec<Index>>,
        lock: Option<&ScopedDirLock>,
    ) -> crate::Result<Log> {
        let create = self.create;

        // Do a lock-less load_or_create_meta to avoid the flock overhead.
        let meta = Log::load_or_create_meta(dir, false).or_else(|err| {
            if create {
                utils::mkdir_p(dir)
                    .context("cannot mkdir after failing to read metadata")
                    .source(err)?;
                // Make sure check and write happens atomically.
                if lock.is_some() {
                    Log::load_or_create_meta(dir, true)
                } else {
                    let _lock = ScopedDirLock::new(&dir)?;
                    Log::load_or_create_meta(dir, true)
                }
            } else {
                Err(err).context(|| format!("cannot open Log at {:?}", &dir))
            }
        })?;

        let mem_buf = Box::pin(Vec::new());
        let (disk_buf, indexes) = Log::load_log_and_indexes(
            Some(dir),
            &meta,
            &self.index_defs,
            &mem_buf,
            reuse_indexes,
            self.fsync,
        )?;
        let mut log = Log {
            dir: Some(dir.to_path_buf()),
            disk_buf,
            mem_buf,
            meta,
            indexes,
            index_corrupted: false,
            open_options: self.clone(),
        };
        log.update_indexes_for_on_disk_entries()?;
        Ok(log)
    }
}

// Repair
impl OpenOptions {
    /// Attempt to repair a broken [`Log`] at the given directory.
    ///
    /// This is done by truncating entries in the primary log, and rebuilding
    /// corrupted indexes.
    ///
    /// Backup files are written for further investigation.
    ///
    /// Return message useful for human consumption.
    pub fn repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        let dir = dir.as_ref();
        let mut message = String::new();
        let result: crate::Result<_> = (|| {
            if !dir.exists() {
                return Ok(format!("{:?} does not exist. Nothing to repair.\n", dir));
            }

            let lock = ScopedDirLock::new(dir)?;

            let primary_path = dir.join(PRIMARY_FILE);
            let meta_path = dir.join(META_FILE);

            // Make sure the header of the primary log file is okay.
            (|| -> crate::Result<()> {
                #[allow(clippy::never_loop)]
                let header_corrupted = loop {
                    if let Err(e) = primary_path.metadata() {
                        if e.kind() == io::ErrorKind::NotFound {
                            break true;
                        }
                    }
                    let mut file = fs::OpenOptions::new()
                        .read(true)
                        .open(&primary_path)
                        .context(&primary_path, "cannot open for read")?;
                    let mut buf = [0; PRIMARY_START_OFFSET as usize];
                    break match file.read_exact(&mut buf) {
                        Ok(_) => buf != PRIMARY_HEADER,
                        Err(_) => true,
                    };
                };
                if header_corrupted {
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .open(&primary_path)
                        .context(&primary_path, "cannot open for write")?;
                    file.write_all(PRIMARY_HEADER)
                        .context(&primary_path, "cannot re-write header")?;
                    let _ = utils::fix_perm_file(&file, false);
                    message += "Fixed header in log\n";
                }
                Ok(())
            })()
            .context("while making sure log has the right header")?;

            // Make sure the "primary_len" is large enough.
            (|| -> crate::Result<()> {
                let primary_len = primary_path
                    .metadata()
                    .context(&primary_path, "cannot read fs metadata")?
                    .len();
                match LogMetadata::read_file(&meta_path)
                    .context(&meta_path, "cannot read log metadata")
                    .context("repair cannot fix metadata corruption")
                {
                    Ok(meta) => {
                        // If metadata can be read, trust it.
                        if meta.primary_len > primary_len {
                            use fs2::FileExt;
                            // Log was truncated for some reason...
                            // (This should be relatively rare)
                            // Fill Log with 0s.
                            let file = fs::OpenOptions::new()
                                .write(true)
                                .open(&primary_path)
                                .context(&primary_path, "cannot open for write")?;
                            file.allocate(meta.primary_len)
                                .context(&primary_path, "cannot fallocate")?;
                            message += &format!(
                                "Extended log to {:?} bytes required by meta\n",
                                meta.primary_len
                            );
                        }
                    }
                    Err(meta_err) => {
                        // Attempt to rebuild metadata.
                        let meta = LogMetadata {
                            primary_len,
                            indexes: BTreeMap::new(),
                            epoch: utils::epoch(),
                        };
                        meta.write_file(&meta_path, self.fsync)
                            .context("while recreating meta")
                            .source(meta_err)?;
                        message += "Rebuilt metadata\n";
                    }
                }
                Ok(())
            })()
            .context("while making sure log.length >= meta.log_length")?;

            // Reload the latest log without indexes.
            //
            // At this time log is likely open-able.
            //
            // Try to open it with indexes so we might reuse them. If that
            // fails, retry with all indexes disabled.
            let mut log = self
                .open_with_lock(dir, &lock)
                .or_else(|_| self.clone().index_defs(Vec::new()).open(dir))
                .context("cannot open log for repair")?;

            let mut iter = log.iter();

            // Read entries until hitting a checksum error.
            let mut entry_count = 0;
            while let Some(Ok(_)) = iter.next() {
                entry_count += 1;
            }

            let valid_len = iter.next_offset;
            assert!(valid_len >= PRIMARY_START_OFFSET);
            assert!(valid_len <= log.meta.primary_len);

            if valid_len == log.meta.primary_len {
                message += &format!(
                    "Verified {} entries, {} bytes in log\n",
                    entry_count, valid_len
                );
            } else {
                message += &format!(
                    "Verified first {} entries, {} of {} bytes in log\n",
                    entry_count, valid_len, log.meta.primary_len
                );

                // Backup the part to be truncated.
                (|| -> crate::Result<()> {
                    let mut primary_file = fs::OpenOptions::new()
                        .read(true)
                        .open(&primary_path)
                        .context(&primary_path, "cannot open for read")?;
                    let backup_path = dir.join(format!(
                        "log.bak.epoch{}.offset{}",
                        log.meta.epoch, valid_len
                    ));
                    let mut backup_file = fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&backup_path)
                        .context(&backup_path, "cannot open")?;

                    primary_file
                        .seek(SeekFrom::Start(valid_len))
                        .context(&primary_path, "cannot seek")?;

                    let mut reader = io::BufReader::new(primary_file);
                    loop {
                        let len = {
                            let buf = reader.fill_buf().context(&primary_path, "cannot read")?;
                            if buf.is_empty() {
                                break;
                            }
                            backup_file
                                .write_all(buf)
                                .context(&backup_path, "cannot write")?;
                            buf.len()
                        };
                        reader.consume(len);
                    }
                    message += &format!("Backed up corrupted log to {:?}\n", backup_path);
                    Ok(())
                })()
                .context("while trying to backup corrupted log")?;

                // Update metadata. Invalidate indexes.
                // Bump epoch since this is a non-append-only change.
                // Reload disk buffer.
                log.meta.primary_len = valid_len;
                log.meta.indexes.clear();
                log.meta.epoch = log.meta.epoch.wrapping_add(1);
                log.disk_buf = Arc::new(mmap_len(&primary_path, valid_len)?);

                log.meta
                    .write_file(&meta_path, log.open_options.fsync)
                    .context("while trying to update metadata with verified log length")?;
                message += &format!("Reset log size to {}\n", valid_len);
            }

            // Also rebuild corrupted indexes.
            // Without this, indexes are empty until the next `sync`, which
            // can lead to bad performance.
            log.open_options.index_defs = self.index_defs.clone();
            message += &log
                .rebuild_indexes_with_lock(false, &lock)
                .context("while trying to update indexes with reapired log")?;

            Ok(message)
        })();

        result.context(|| format!("in log::OpenOptions::repair({:?})", dir))
    }
}

impl OpenOptionsRepair for OpenOptions {
    fn open_options_repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        OpenOptions::repair(self, dir.as_ref())
    }
}

impl OpenOptions {
    /// Attempt to change a [`Log`] at the given directory so it becomes
    /// empty and hopefully recovers from some corrupted state.
    ///
    /// Warning: This deletes data, and there is no backup!
    pub fn delete_content(&self, dir: impl AsRef<Path>) -> crate::Result<()> {
        let dir = dir.as_ref();
        let result: crate::Result<()> = (|| {
            // Ensure the directory exist.
            utils::mkdir_p(dir)?;

            // Prevent other writers.
            let lock = ScopedDirLock::new(dir)?;

            // Replace the metadata to an empty state.
            let meta = LogMetadata {
                primary_len: PRIMARY_START_OFFSET,
                indexes: BTreeMap::new(),
                epoch: utils::epoch(),
            };
            let meta_path = dir.join(META_FILE);
            meta.write_file(&meta_path, self.fsync)?;

            // Replace the primary log.
            let primary_path = dir.join(PRIMARY_FILE);
            atomic_write(primary_path, PRIMARY_HEADER, self.fsync)?;

            // Replace indexes so they become empty.
            let log = self
                .clone()
                .create(true)
                .open_with_lock(dir, &lock)
                .context("cannot open")?;
            log.rebuild_indexes_with_lock(true, &lock)?;

            Ok(())
        })();

        result.context(|| format!("in log::OpenOptions::delete_content({:?})", dir))
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
    type Item = crate::Result<&'a [u8]>;

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
            Some(Ok(offset)) => match self
                .log
                .read_entry(offset)
                .context("in LogLookupIter::next")
            {
                Ok(Some(entry)) => Some(Ok(entry.data)),
                Ok(None) => None,
                Err(err) => {
                    // Do not set this iterator to an error state. It's possible
                    // that the index iterator still provides valid data, and
                    // only the "log" portion is corrupted.
                    //
                    // The index iterator is finite if integrity check is turned
                    // on. So trust it and don't worry about infinite iteration
                    // here.
                    Some(Err(err))
                }
            },
        }
    }
}

impl<'a> LogLookupIter<'a> {
    /// A convenient way to get data.
    pub fn into_vec(self) -> crate::Result<Vec<&'a [u8]>> {
        self.collect()
    }
}

impl<'a> Iterator for LogIter<'a> {
    type Item = crate::Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        match self
            .log
            .read_entry(self.next_offset)
            .context("in LogIter::next")
        {
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
        item: Option<crate::Result<(Cow<'a, [u8]>, index::LinkOffset)>>,
    ) -> Option<crate::Result<(Cow<'a, [u8]>, LogLookupIter<'a>)>> {
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
    type Item = crate::Result<(Cow<'a, [u8]>, LogLookupIter<'a>)>;

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

impl IndexOutput {
    fn into_cow(self, data: &[u8]) -> crate::Result<Cow<[u8]>> {
        Ok(match self {
            IndexOutput::Reference(range) => Cow::Borrowed(
                &data
                    .get(range.start as usize..range.end as usize)
                    .ok_or_else(|| {
                        let msg = format!(
                            "IndexFunc returned range {:?} but the data only has {} bytes",
                            range,
                            data.len()
                        );
                        let mut err = crate::Error::programming(msg);
                        // If the data is short, add its content to error message.
                        if data.len() < 128 {
                            err = err.message(format!("Data = {:?}", data))
                        }
                        err
                    })?,
            ),
            IndexOutput::Owned(key) => Cow::Owned(key.into_vec()),
            IndexOutput::Remove(_) | IndexOutput::RemovePrefix(_) => {
                return Err(crate::Error::programming(
                    "into_cow does not support Remove or RemovePrefix",
                ))
            }
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
                Some(Ok(bytes)) => writeln!(f, "{{ bytes: {:?} }}", bytes)?,
                Some(Err(err)) => writeln!(f, "{{ error: {:?} }}", err)?,
            }
        }
        Ok(())
    }
}

impl ReadonlyBuffer for ExternalKeyBuffer {
    #[inline]
    fn slice(&self, start: u64, len: u64) -> &[u8] {
        if start < self.disk_len {
            &self.disk_buf[(start as usize)..(start + len) as usize]
        } else {
            let start = start - self.disk_len;
            // See "UNSAFE NOTICE" in ExternalKeyBuffer definition.
            // This pointer cannot be null.
            let mem_buf = unsafe { &*self.mem_buf };
            &mem_buf[(start as usize)..(start + len) as usize]
        }
    }
}

impl fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "OpenOptions {{ ")?;
        write!(
            f,
            "index_defs: {:?}, ",
            self.index_defs.iter().map(|d| d.name).collect::<Vec<_>>()
        )?;
        write!(f, "fsync: {}, ", self.fsync)?;
        write!(f, "create: {}, ", self.create)?;
        write!(f, "checksum_type: {:?}, ", self.checksum_type)?;
        write!(f, "auto_sync_threshold: {:?}, ", self.auto_sync_threshold)?;
        let flush_filter_desc = match self.flush_filter {
            Some(ref _buf) => "Some(_)",
            None => "None",
        };
        write!(f, "flush_filter: {} }}", flush_filter_desc)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use std::cell::RefCell;
    use tempfile::tempdir;

    #[derive(Debug)]
    struct DummyError(&'static str);

    impl fmt::Display for DummyError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for DummyError {}

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
    fn test_incomplete_rewrite() {
        let dir = tempdir().unwrap();
        let read_entries = || -> Vec<Vec<u8>> {
            let log = Log::open(&dir, Vec::new()).unwrap();
            log.iter()
                .map(|v| v.map(|v| v.to_vec()))
                .collect::<Result<Vec<Vec<u8>>, _>>()
                .unwrap()
        };
        let add_noise = |noise: &[u8]| {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(dir.path().join("log"))
                .unwrap();
            // Emulate an incomplete write with broken data.
            file.write_all(noise).unwrap();
        };

        let mut log1 = Log::open(&dir, Vec::new()).unwrap();
        log1.append(b"abc").unwrap();
        log1.sync().unwrap();
        assert_eq!(read_entries(), vec![b"abc"]);

        add_noise(&[0xcc; 1]);
        assert_eq!(read_entries(), vec![b"abc"]);

        log1.append(b"def").unwrap();
        log1.sync().unwrap();
        assert_eq!(read_entries(), vec![b"abc", b"def"]);

        add_noise(&[0xcc; 1000]);
        assert_eq!(read_entries(), vec![b"abc", b"def"]);

        log1.append(b"ghi").unwrap();
        log1.sync().unwrap();
        assert_eq!(read_entries(), vec![b"abc", b"def", b"ghi"]);

        add_noise(&[0xcc; 1000]);
        assert_eq!(read_entries(), vec![b"abc", b"def", b"ghi"]);
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
        assert_eq!(log.sync().unwrap(), 486);

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
            log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
            vec![b"2", b"4", b"3"]
        );
        assert_eq!(
            log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
            log.iter_dirty().collect::<crate::Result<Vec<_>>>().unwrap(),
        );

        log.sync().unwrap();

        assert!(log
            .iter_dirty()
            .collect::<crate::Result<Vec<_>>>()
            .unwrap()
            .is_empty());
        assert_eq!(
            log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
            vec![b"2", b"4", b"3"]
        );

        log.append(b"5").unwrap();
        log.append(b"1").unwrap();
        assert_eq!(
            log.iter_dirty().collect::<crate::Result<Vec<_>>>().unwrap(),
            vec![b"5", b"1"]
        );
        assert_eq!(
            log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
            vec![b"2", b"4", b"3", b"5", b"1"]
        );
    }

    fn get_index_defs(lag_threshold: u64) -> Vec<IndexDef> {
        // Two index functions. First takes every 2 bytes as references. The second takes every 3
        // bytes as owned slices.
        // Keys starting with '-' are considered as "deletion" requests.
        // Keys starting with '=' are considered as "delete prefix" requests.
        let index_func0 = |data: &[u8]| {
            if data.first() == Some(&b'=') {
                return vec![IndexOutput::RemovePrefix(
                    data[1..].to_vec().into_boxed_slice(),
                )];
            }
            let is_removal = data.first() == Some(&b'-');
            let start = if is_removal { 1 } else { 0 };
            (start..(data.len().max(1) - 1))
                .map(|i| {
                    if is_removal {
                        IndexOutput::Remove(data[i..i + 2].to_vec().into_boxed_slice())
                    } else {
                        IndexOutput::Reference(i as u64..i as u64 + 2)
                    }
                })
                .collect()
        };
        let index_func1 = |data: &[u8]| {
            if data.first() == Some(&b'=') {
                return vec![IndexOutput::RemovePrefix(
                    data[1..].to_vec().into_boxed_slice(),
                )];
            }
            let is_removal = data.first() == Some(&b'-');
            let start = if is_removal { 1 } else { 0 };
            (start..(data.len().max(2) - 2))
                .map(|i| {
                    let bytes = data[i..i + 3].to_vec().into_boxed_slice();
                    if is_removal {
                        IndexOutput::Remove(bytes)
                    } else {
                        IndexOutput::Owned(bytes)
                    }
                })
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
            let entries: [&[u8]; 7] = [b"1", b"", b"2345", b"", b"78", b"3456", b"35"];
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

            log.sync().unwrap();

            // Delete prefix.
            log.append(b"=3").unwrap();
            for key in [b"34", b"35"].iter() {
                assert!(log.lookup(0, key).unwrap().into_vec().unwrap().is_empty());
            }
            assert_eq!(log.lookup(0, b"56").unwrap().into_vec().unwrap(), [b"3456"]);

            // Delete keys.
            let mut log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
            for bytes in entries.iter() {
                let mut bytes = bytes.to_vec();
                bytes.insert(0, b'-');
                log.append(&bytes).unwrap();
                if bytes.is_empty() {
                    log.sync().expect("flush");
                    log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
                }
            }
            for key in [b"34", b"56", b"78"].iter() {
                assert!(log.lookup(0, key).unwrap().into_vec().unwrap().is_empty());
            }
            assert_eq!(log.lookup(1, b"345").unwrap().count(), 0);
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
                    4 => return Err(Box::new(DummyError("error"))),
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

    /// Get a `Log` with index defined on first 8 bytes.
    fn log_with_index(path: &Path, lag: u64) -> Log {
        let index_func = |_data: &[u8]| vec![IndexOutput::Reference(0..8)];
        let index_def = IndexDef::new("i", index_func).lag_threshold(lag);
        Log::open(path, vec![index_def]).unwrap()
    }

    /// Insert entries to a log
    fn insert_entries(log: &mut Log, start: u64, n: u64) {
        for i in start..(start + n) {
            let buf: [u8; 8] = unsafe { std::mem::transmute(i as u64) };
            log.append(&buf[..]).unwrap();
        }
    }

    #[test]
    fn test_sync_fast_paths() {
        // Make sure various "sync" code paths do not lose data.
        //
        // Include these paths:
        //
        // - log1 and log2 are created.
        // - log1 writes (choice1)
        //   - 1: with index lag = 0
        //   - 2: with index lag = large value
        //   - 3: skip this step
        // - log1 sync()
        // - log2 writes (choice2)
        //   - 4: with index lag = 0
        //   - 5: with index lag = large value
        //   - 6: skip this step
        // - log2 sync()
        // - log1 sync()
        //
        // Examine log2 and log1 indexes by counting the entries in the log
        // and the index.

        const N: u64 = 1003;

        for choice1 in vec![1, 2, 3] {
            for choice2 in vec![4, 5, 6] {
                let dir = tempdir().unwrap();
                // Write a single entry to make the log non-empty.
                // So it's slightly more interesting.
                let mut log0 = log_with_index(dir.path(), 0);
                log0.sync().unwrap();

                let mut log1 = log_with_index(dir.path(), (choice1 - 1) << 29);
                let mut log2 = log_with_index(dir.path(), (choice2 - 4) << 29);
                let mut count = 0usize;

                if choice1 < 3 {
                    count += N as usize;
                    insert_entries(&mut log1, 0, N);
                }
                log1.sync().unwrap();

                if choice2 < 6 {
                    count += (N as usize) * 2;
                    insert_entries(&mut log2, N, N * 2);
                }
                log2.sync().unwrap();
                log1.sync().unwrap();

                let s = format!("(choices = {} {})", choice1, choice2);
                assert_eq!(
                    log1.lookup_range(0, ..).unwrap().count(),
                    count,
                    "log1 index is incomplete {}",
                    s
                );
                assert_eq!(
                    log2.lookup_range(0, ..).unwrap().count(),
                    count,
                    "log2 index is incomplete {}",
                    s
                );
                assert_eq!(log1.iter().count(), count, "log1 log is incomplete {}", s);
                assert_eq!(log2.iter().count(), count, "log2 log is incomplete {}", s);
            }
        }
    }

    #[test]
    fn test_auto_sync_threshold() {
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new().create(true).auto_sync_threshold(100);
        let mut log = open_opts.open(&dir).unwrap();
        log.append(vec![b'a'; 50]).unwrap();
        assert_eq!(log.iter_dirty().count(), 1);

        log.append(vec![b'b'; 50]).unwrap(); // trigger auto-sync
        assert_eq!(log.iter_dirty().count(), 0);
    }

    #[test]
    fn test_sync_missing_meta() {
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new().create(true);
        let mut log = open_opts.open(&dir).unwrap();
        log.append(vec![b'a'; 100]).unwrap();
        log.sync().unwrap();

        let mut log2 = open_opts.open(&dir.path()).unwrap();
        fs::remove_file(&dir.path().join(META_FILE)).unwrap();
        log2.sync().unwrap(); // pretend to be a no-op

        log2.append(vec![b'b'; 100]).unwrap();
        log2.sync().unwrap_err(); // an error
    }

    fn test_rebuild_indexes() {
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new()
            .create(true)
            .index_defs(vec![IndexDef::new("key", |data| {
                vec![IndexOutput::Reference(0..data.len() as u64)]
            })
            .lag_threshold(1)]);
        let mut log = open_opts.clone().open(dir.path()).unwrap();

        log.append(b"abc").unwrap();
        log.flush().unwrap();

        log.append(b"def").unwrap();
        log.flush().unwrap();

        let dump_index = || {
            let index = index::OpenOptions::new()
                .open(dir.path().join("index-key"))
                .unwrap();
            format!("{:?}", index)
        };

        let dump1 = dump_index();
        assert_eq!(
            dump1,
            "Index { len: 53, root: Disk[40] }\n\
             Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }\n\
             Disk[2]: ExtKey { start: 18, len: 3 }\n\
             Disk[5]: Link { value: 12, next: None }\n\
             Disk[8]: Radix { link: None, 6: Disk[1] }\n\
             Disk[16]: Root { radix: Disk[8], meta: [21] }\n\
             Disk[21]: InlineLeaf { key: Disk[22], link: Disk[25] }\n\
             Disk[22]: ExtKey { start: 27, len: 3 }\n\
             Disk[25]: Link { value: 21, next: None }\n\
             Disk[28]: Radix { link: None, 1: Disk[1], 4: Disk[21] }\n\
             Disk[40]: Radix { link: None, 6: Disk[28] }\n\
             Disk[48]: Root { radix: Disk[40], meta: [30] }\n"
        );

        // If force is false, it is a no-op since the index passes the
        // checksum check.
        log.try_clone().unwrap().rebuild_indexes(false).unwrap();
        assert_eq!(dump_index(), dump1);

        // Setting force to true to rebuild the index.
        log.rebuild_indexes(true).unwrap();

        // The rebuilt index only contains one Root.
        assert_eq!(
            dump_index(),
            "Index { len: 40, root: Disk[27] }\n\
             Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }\n\
             Disk[2]: ExtKey { start: 18, len: 3 }\n\
             Disk[5]: Link { value: 12, next: None }\n\
             Disk[8]: InlineLeaf { key: Disk[9], link: Disk[12] }\n\
             Disk[9]: ExtKey { start: 27, len: 3 }\n\
             Disk[12]: Link { value: 21, next: None }\n\
             Disk[15]: Radix { link: None, 1: Disk[1], 4: Disk[8] }\n\
             Disk[27]: Radix { link: None, 6: Disk[15] }\n\
             Disk[35]: Root { radix: Disk[27], meta: [30] }\n"
        );

        // The index actually works (checksum table is consistent).
        let log = open_opts.open(dir.path()).unwrap();
        assert_eq!(log.lookup(0, b"abc").unwrap().count(), 1);
        assert_eq!(log.lookup(0, b"def").unwrap().count(), 1);
        assert_eq!(log.lookup(0, b"xyz").unwrap().count(), 0);
    }

    fn pwrite(path: &Path, offset: i64, data: &[u8]) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open(path)
            .unwrap();
        if offset < 0 {
            file.seek(SeekFrom::End(offset)).unwrap();
        } else {
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
        }
        file.write_all(data).unwrap();
    }

    #[test]
    fn test_repair() {
        let dir = tempdir().unwrap();
        {
            let mut log = Log::open(dir.path(), Vec::new()).unwrap();
            log.append(b"abc").unwrap();
            log.append(b"def").unwrap();
            log.append(b"ghi").unwrap();
            log.flush().unwrap();
        }

        // Corrupt the log by changing the last byte.
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .read(true)
                .open(dir.path().join(PRIMARY_FILE))
                .unwrap();
            file.seek(SeekFrom::End(-1)).unwrap();
            file.write_all(b"x").unwrap();
        }

        // Reading entries would error out.
        {
            let log = Log::open(dir.path(), Vec::new()).unwrap();
            assert!(log.iter().nth(2).unwrap().is_err());
        }

        // Repair.
        {
            OpenOptions::new().repair(dir.path()).unwrap();
        }

        // Reading entries is recovered. But we lost one entry.
        let mut log = Log::open(dir.path(), Vec::new()).unwrap();
        assert_eq!(
            log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![b"abc", b"def"]
        );

        // Writing is recovered.
        log.append(b"pqr").unwrap();
        log.flush().unwrap();

        let log = Log::open(dir.path(), Vec::new()).unwrap();
        assert_eq!(
            log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![b"abc", b"def", b"pqr"]
        );
    }

    #[test]
    fn test_repair_noop() {
        // Repair does nothing if the Log can be read out without issues.
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), Vec::new()).unwrap();
        log.append(b"abc").unwrap();
        log.append(b"def").unwrap();
        log.append(b"ghi").unwrap();
        log.flush().unwrap();

        let meta_before = LogMetadata::read_file(dir.path().join(META_FILE)).unwrap();
        OpenOptions::new().repair(dir.path()).unwrap();
        let meta_after = LogMetadata::read_file(dir.path().join(META_FILE)).unwrap();
        assert_eq!(meta_before, meta_after);
    }

    #[test]
    fn test_repair_and_delete_content() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        let open_opts = OpenOptions::new()
            .create(true)
            .index("c", |_| vec![IndexOutput::Reference(0..1)]);

        let long_lived_log = RefCell::new(open_opts.create_in_memory().unwrap());
        let open = || open_opts.open(&path);
        let corrupt = |name: &str, offset: i64| pwrite(&path.join(name), offset, b"cc");
        let truncate = |name: &str| fs::write(path.join(name), "garbage").unwrap();
        let delete = |name: &str| fs::remove_file(path.join(name)).unwrap();
        let index_file = format!("{}c", INDEX_FILE_PREFIX);
        let checksum_file = format!("{}c.sum", INDEX_FILE_PREFIX);
        let append = || {
            let mut log = open().unwrap();
            log.append(&[b'x'; 50_000][..]).unwrap();
            log.append(&[b'y'; 50_000][..]).unwrap();
            log.append(&[b'z'; 50_000][..]).unwrap();
            log.sync().unwrap();
        };
        let count = || -> crate::Result<(usize, usize)> {
            let log = open()?;
            let log_len = log.iter().collect::<Result<Vec<_>, _>>()?.len();
            let mut index_len = 0;
            for key in [b"x", b"y", b"z"].iter() {
                let iter = log.lookup(0, key)?;
                index_len += iter.into_vec()?.len();
            }
            Ok((log_len, index_len))
        };
        let verify_len = |len: usize| {
            let (log_len, index_len) = count().unwrap();
            assert_eq!(log_len, len);
            assert_eq!(index_len, len);
        };
        let verify_corrupted = || {
            let err = count().unwrap_err();
            assert!(err.is_corruption(), "not a corruption:\n {:?}", err);
        };
        let try_trigger_sigbus = || {
            // Check no SIGBUS
            let log = long_lived_log.borrow();
            match log.lookup(0, "z") {
                Err(_) => (), // okay - not SIGBUS
                Ok(iter) => match iter.into_vec() {
                    Err(_) => (), // okay - not SIGBUS
                    Ok(_) => (),  // okay - not SIGBUS
                },
            }
            // Check 'sync' on a long-lived log will load the right data and
            // resolve errors.
            let mut cloned_log = log.try_clone().unwrap();
            cloned_log.sync().unwrap();
            let _ = cloned_log.lookup(0, "z").unwrap().into_vec().unwrap();
        };
        let repair = || {
            let message = open_opts.repair(&path).unwrap();
            try_trigger_sigbus();
            message
                .lines()
                // Remove 'Backed up' lines since they have dynamic file names.
                .filter(|l| !l.contains("Backed up"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Repair is a no-op if log and indexes pass integirty check.
        append();
        verify_len(3);
        assert_eq!(
            repair(),
            r#"Verified 3 entries, 150048 bytes in log
Index "c" passed integrity check"#
        );

        append();
        verify_len(6);
        assert_eq!(
            repair(),
            r#"Verified 6 entries, 300084 bytes in log
Index "c" passed integrity check"#
        );

        // Prepare long-lived log for SIGBUS check
        // (skip on Windows, since mmap makes it impossible to replace files)
        if cfg!(unix) {
            long_lived_log.replace(open().unwrap());
        }

        // Corrupt the end of log
        corrupt(PRIMARY_FILE, -1);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified first 5 entries, 250072 of 300084 bytes in log
Reset log size to 250072
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
        );
        verify_len(5);

        // Corrupt the middle of log
        corrupt(PRIMARY_FILE, 125000);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified first 2 entries, 100036 of 250072 bytes in log
Reset log size to 100036
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
        );
        verify_len(2);

        append();
        verify_len(5);

        // Change the beginning of log
        corrupt(PRIMARY_FILE, 1);
        verify_len(5);
        assert_eq!(
            repair(),
            r#"Fixed header in log
Verified 5 entries, 250072 bytes in log
Index "c" passed integrity check"#
        );

        // Corrupt the end of index
        corrupt(&index_file, -1);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
        );
        verify_len(5);

        // Corrupt the beginning of index
        corrupt(&index_file, 1);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
        );
        verify_len(5);

        // Corrupt index checksum
        corrupt(&checksum_file, -2);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
        );
        verify_len(5);

        // Replace index with garbage
        truncate(&index_file);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
        );
        verify_len(5);

        // Replace index checksum with garbage
        truncate(&checksum_file);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
        );
        verify_len(5);

        // Replace log with garbage
        truncate(PRIMARY_FILE);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Fixed header in log
Extended log to 250072 bytes required by meta
Verified first 0 entries, 12 of 250072 bytes in log
Reset log size to 12
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
        );
        verify_len(0);

        append();
        verify_len(3);

        // Delete index
        delete(&index_file);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 3 entries, 150048 bytes in log
Rebuilt index "c""#
        );
        verify_len(3);

        // Delete checksum
        delete(&checksum_file);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 3 entries, 150048 bytes in log
Rebuilt index "c""#
        );
        verify_len(3);

        // Delete log
        delete(PRIMARY_FILE);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Fixed header in log
Extended log to 150048 bytes required by meta
Verified first 0 entries, 12 of 150048 bytes in log
Reset log size to 12
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
        );
        verify_len(0);

        // Corrupt the middle of index. This test wants to be able
        // to make it okay to open Index, but not okay to use it at
        // some random place. The index checksum chunk size is 1MB
        // so the index has to be a few MBs to be able to pass checksum
        // check at Index open time.
        // To do that, insert a lot entries to the log.
        //
        // Practically, this should show "Index .. failed integrity check".
        let append_many_entries = || {
            let mut log = open().unwrap();
            for _ in 0..200_000 {
                log.append(&[b'z'; 1][..]).unwrap();
            }
            log.sync().unwrap();
        };
        append_many_entries();
        corrupt(&index_file, -1000_000);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Verified 200000 entries, 1400012 bytes in log
Index "c" failed integrity check
Rebuilt index "c""#
        );
        verify_len(200000);

        // Corrupt meta
        corrupt(META_FILE, 2);
        corrupt(PRIMARY_FILE, 1000);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Rebuilt metadata
Verified first 141 entries, 999 of 1400012 bytes in log
Reset log size to 999
Rebuilt index "c""#
        );
        verify_len(141);

        truncate(META_FILE);
        verify_corrupted();
        assert_eq!(
            repair(),
            r#"Rebuilt metadata
Verified first 141 entries, 999 of 1400012 bytes in log
Reset log size to 999
Rebuilt index "c""#
        );
        verify_len(141);

        // Delete meta - as if the log directory does not exist.
        delete(META_FILE);
        assert_eq!(
            repair(),
            r#"Rebuilt metadata
Verified first 141 entries, 999 of 1400012 bytes in log
Reset log size to 999
Rebuilt index "c""#
        );
        verify_len(141);

        let len = |name: &str| path.join(name).metadata().unwrap().len();
        let append = || {
            let mut log = open().unwrap();
            log.append(&[b'x'; 50_000][..]).unwrap();
            log.append(&[b'y'; 50_000][..]).unwrap();
            log.append(&[b'z'; 50_000][..]).unwrap();
            log.sync().unwrap();
            assert_eq!(len(PRIMARY_FILE), PRIMARY_START_OFFSET + 150036);
            assert_eq!(len(&index_file), 70);
        };
        let delete_content = || {
            open_opts.delete_content(&path).unwrap();
            assert_eq!(len(PRIMARY_FILE), PRIMARY_START_OFFSET);
            assert_eq!(len(&index_file), 10);
            // Check SIGBUS
            try_trigger_sigbus();
            // Check log is empty
            verify_len(0);
        };

        // 'dir' does not exist - delete_content creates the log
        fs::remove_dir_all(&path).unwrap();
        delete_content();

        // Empty log
        delete_content();

        // Normal log
        append();
        if cfg!(unix) {
            long_lived_log.replace(open().unwrap());
        }
        delete_content();

        // Corrupt log
        append();
        corrupt(PRIMARY_FILE, -75_000);
        delete_content();

        // Corrupt index
        append();
        corrupt(&index_file, -10);
        delete_content();

        // Corrupt checksum
        append();
        corrupt(&checksum_file, -10);
        delete_content();

        // Corrupt log and index
        append();
        corrupt(PRIMARY_FILE, -25_000);
        corrupt(&index_file, -10);
        delete_content();

        // Deleted various files
        delete(&checksum_file);
        delete_content();

        delete(&index_file);
        delete_content();

        delete(PRIMARY_FILE);
        delete_content();

        delete(META_FILE);
        delete_content();
    }

    #[test]
    fn test_zero_data() {
        // Emulating the case where meta was written, but log was zeroed out.
        // This should be captured by checksums.
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), Vec::new()).unwrap();
        log.append(b"abcd").unwrap();
        log.flush().unwrap();

        let len_before = dir.path().join(PRIMARY_FILE).metadata().unwrap().len();
        log.append(b"efgh").unwrap();
        log.flush().unwrap();

        let len_after = dir.path().join(PRIMARY_FILE).metadata().unwrap().len();

        // Zero-out the second entry
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .read(true)
                .open(dir.path().join(PRIMARY_FILE))
                .unwrap();
            file.seek(SeekFrom::Start(len_before)).unwrap();
            file.write_all(&vec![0; (len_after - len_before) as usize])
                .unwrap();
        }

        let log = Log::open(dir.path(), Vec::new()).unwrap();
        assert!(log.iter().any(|e| e.is_err()));
    }

    #[cfg(unix)]
    #[test]
    fn test_non_append_only() {
        // Test non-append-only changes can be detected by epoch change.
        //
        // In this test, we create 2 logs with different content. Then swap
        // those 2 logs and call sync.
        //
        // This test requires renaming files while mmap is present. That
        // cannot be done in Windows.
        //
        // This test should fail if utils::epoch returns a constant.
        let dir = tempdir().unwrap();

        let indexes = vec![IndexDef::new("key1", index_ref).lag_threshold(1)];
        let open_opts = OpenOptions::new().create(true).index_defs(indexes);

        // Prepare the first log.
        let mut log1 = open_opts.open(dir.path().join("1")).unwrap();
        for b in 0..10 {
            log1.append(&[b; 7][..]).unwrap();
        }
        log1.flush().unwrap();
        for b in 30..40 {
            log1.append(&[b; 21][..]).unwrap();
        }

        // Prepare the second log
        let mut log2 = open_opts.open(dir.path().join("2")).unwrap();
        for b in 20..30 {
            log2.append(&[b; 21][..]).unwrap();
        }
        log2.flush().unwrap();
        for b in 10..20 {
            log2.append(&[b; 7][..]).unwrap();
        }

        // Rename to emulate the non-append-only change.
        fs::rename(dir.path().join("1"), dir.path().join("temp")).unwrap();
        fs::rename(dir.path().join("2"), dir.path().join("1")).unwrap();
        fs::rename(dir.path().join("temp"), dir.path().join("2")).unwrap();

        log1.sync().unwrap();
        log2.sync().unwrap();

        // Check their content.
        fn check_log(log: &Log, range: Range<u8>, len: usize) {
            assert_eq!(
                log.iter().map(|b| b.unwrap().to_vec()).collect::<Vec<_>>(),
                range.clone().map(|i| vec![i; len]).collect::<Vec<_>>(),
            );
            assert_eq!(
                log.lookup_range(0, ..)
                    .unwrap()
                    .flat_map(|e| e.unwrap().1.into_vec().unwrap())
                    .map(|b| b.to_vec())
                    .collect::<Vec<_>>(),
                range.map(|i| vec![i; len]).collect::<Vec<_>>(),
            );
        }

        check_log(&log1, 20..40, 21);
        check_log(&log2, 0..20, 7);

        let log1 = open_opts.open(dir.path().join("1")).unwrap();
        let log2 = open_opts.open(dir.path().join("2")).unwrap();

        check_log(&log1, 20..40, 21);
        check_log(&log2, 0..20, 7);
    }

    #[test]
    fn test_clear_dirty() {
        for lag in vec![0, 1000] {
            let dir = tempdir().unwrap();
            let mut log = log_with_index(dir.path(), lag);
            log.append([b'a'; 10]).unwrap();
            log.sync().unwrap();
            log.append([b'b'; 10]).unwrap();
            assert_eq!(log.lookup_range(0, ..).unwrap().count(), 2);

            log.clear_dirty().unwrap();
            assert_eq!(
                log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
                vec![[b'a'; 10]],
            );
            assert_eq!(log.lookup_range(0, ..).unwrap().count(), 1);
        }
    }

    #[test]
    fn test_clone() {
        for lag in vec![0, 1000] {
            let dir = tempdir().unwrap();
            let mut log = log_with_index(dir.path(), lag);
            log.append([b'a'; 10]).unwrap();
            log.sync().unwrap();
            log.append([b'b'; 10]).unwrap();

            let log2 = log.try_clone().unwrap();
            assert_eq!(log2.iter().collect::<Result<Vec<_>, _>>().unwrap().len(), 2);
            assert_eq!(log2.lookup_range(0, ..).unwrap().count(), 2);

            let log2 = log.try_clone_without_dirty().unwrap();
            assert_eq!(log2.iter().collect::<Result<Vec<_>, _>>().unwrap().len(), 1);
            assert_eq!(log2.lookup_range(0, ..).unwrap().count(), 1);
        }
    }

    #[test]
    fn test_multithread_sync() {
        let dir = tempdir().unwrap();

        const THREAD_COUNT: u8 = 30;

        // Release mode runs much faster.
        #[cfg(debug_assertions)]
        const WRITE_COUNT_PER_THREAD: u8 = 30;
        #[cfg(not(debug_assertions))]
        const WRITE_COUNT_PER_THREAD: u8 = 150;

        // Some indexes. They have different lag_threshold.
        fn index_copy(data: &[u8]) -> Vec<IndexOutput> {
            vec![IndexOutput::Owned(data.to_vec().into_boxed_slice())]
        }
        let indexes = vec![
            IndexDef::new("key1", index_ref).lag_threshold(1),
            IndexDef::new("key2", index_ref).lag_threshold(50),
            IndexDef::new("key3", index_ref).lag_threshold(1000),
            IndexDef::new("key4", index_copy).lag_threshold(1),
            IndexDef::new("key5", index_copy).lag_threshold(50),
            IndexDef::new("key6", index_copy).lag_threshold(1000),
        ];
        let index_len = indexes.len();
        let open_opts = OpenOptions::new().create(true).index_defs(indexes);

        let barrier = Arc::new(std::sync::Barrier::new(THREAD_COUNT as usize));
        let threads: Vec<_> = (0..THREAD_COUNT)
            .map(|i| {
                let barrier = barrier.clone();
                let open_opts = open_opts.clone();
                let path = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut log = open_opts.open(path).unwrap();
                    for j in 1..=WRITE_COUNT_PER_THREAD {
                        let buf = [i, j];
                        log.append(&buf).unwrap();
                        if j % (i + 1) == 0 || j == WRITE_COUNT_PER_THREAD {
                            log.sync().unwrap();
                            // Verify that the indexes match the entries.
                            let mut entries =
                                log.iter().map(|d| d.unwrap().to_vec()).collect::<Vec<_>>();
                            entries.sort_unstable();
                            for index_id in 0..index_len {
                                let mut entries_index_keys = Vec::with_capacity(entries.len());
                                let mut entries_index_values = Vec::with_capacity(entries.len());
                                for entry_iter in log.lookup_range(index_id, ..).unwrap() {
                                    let (key, value_iter) = entry_iter.unwrap();
                                    entries_index_keys.push(key.as_ref().to_vec());
                                    for value in value_iter {
                                        entries_index_values.push(value.unwrap().to_vec());
                                    }
                                }
                                assert_eq!(entries, entries_index_keys);
                                assert_eq!(entries, entries_index_values);
                            }
                        }
                    }
                })
            })
            .collect();

        // Wait for them.
        for thread in threads {
            thread.join().expect("joined");
        }

        // Check how many entries were written.
        let log = open_opts.open(dir.path()).unwrap();
        let count = log.iter().count() as u64;
        assert_eq!(count, THREAD_COUNT as u64 * WRITE_COUNT_PER_THREAD as u64);
    }

    fn index_ref(data: &[u8]) -> Vec<IndexOutput> {
        vec![IndexOutput::Reference(0..data.len() as u64)]
    }

    quickcheck! {
        fn test_roundtrip_entries(entries: Vec<(Vec<u8>, bool, bool)>) -> bool {
            let dir = tempdir().unwrap();
            let mut log = Log::open(dir.path(), Vec::new()).unwrap();
            let mut log_mem = OpenOptions::new().create_in_memory().unwrap();
            for &(ref data, flush, reload) in &entries {
                log.append(data).expect("append");
                log_mem.append(data).expect("append");
                if flush {
                    log.sync().expect("flush");
                    if reload {
                        log = Log::open(dir.path(), Vec::new()).unwrap();
                    }
                }
            }
            let retrieved: Vec<Vec<u8>> = log.iter().map(|v| v.unwrap().to_vec()).collect();
            let retrieved_mem: Vec<Vec<u8>> = log_mem.iter().map(|v| v.unwrap().to_vec()).collect();
            let entries: Vec<Vec<u8>> = entries.iter().map(|v| v.0.clone()).collect();
            retrieved == entries && retrieved_mem == entries
        }
    }
}
