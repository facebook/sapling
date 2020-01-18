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
use crate::utils::{self, mmap_empty, mmap_len, xxhash, xxhash32};
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use memmap::Mmap;
use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::ops::RangeBounds;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tracing::debug_span;
use vlqencoding::{VLQDecodeAt, VLQEncode};

mod meta;
mod open_options;
mod path;
mod repair;
#[cfg(test)]
mod tests;

pub use self::meta::LogMetadata;
pub use open_options::{
    ChecksumType, FlushFilterContext, FlushFilterFunc, FlushFilterOutput, IndexDef, IndexOutput,
    OpenOptions,
};
pub use path::GenericPath;

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
    pub dir: GenericPath,
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
            .open(dir.as_ref())
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
            Box::pin(Vec::new())
        };

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
            if let Some(dir) = &self.dir.as_opt_path() {
                span.record("dir", &dir.to_string_lossy().as_ref());
            }
            let _guard = span.enter();

            if self.dir.as_opt_path().is_none() {
                // See Index::flush for why this is not an Err.
                return Ok(0);
            }

            fn check_append_only(this: &Log, new_meta: &LogMetadata) -> crate::Result<()> {
                let old_meta = &this.meta;
                if old_meta.primary_len > new_meta.primary_len {
                    Err(crate::Error::path(this.dir.as_opt_path().unwrap(), format!(
                    "on-disk log is unexpectedly smaller ({} bytes) than its previous version ({} bytes)",
                    new_meta.primary_len, old_meta.primary_len
                )))
                } else {
                    Ok(())
                }
            }

            // Read-only fast path - no need to take directory lock.
            if self.mem_buf.is_empty() {
                if let Ok(meta) = Self::load_or_create_meta(&self.dir, false) {
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
                            &self.dir,
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
            let dir = self.dir.as_opt_path().unwrap().to_path_buf();
            let lock = ScopedDirLock::new(&dir)?;

            // Step 1: Reload metadata to get the latest view of the files.
            let mut meta = Self::load_or_create_meta(&self.dir, false)?;
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
                    .open_with_lock(&self.dir, &lock)
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
                    .open_with_lock(&self.dir, &lock)
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
            let primary_path = self.dir.as_opt_path().unwrap().join(PRIMARY_FILE);
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
                &self.dir,
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
            self.dir.write_meta(&self.meta, self.open_options.fsync)?;

            Ok(self.meta.primary_len)
        })();

        result
            .context("in Log::sync")
            .context(|| format!("  Log.dir = {:?}", self.dir))
    }

    /// Check if the log is changed on disk.
    pub fn is_changed(&self) -> bool {
        match self.dir.read_meta() {
            Ok(meta) => meta != self.meta,
            Err(_) => true,
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
            let dir = self.dir.clone();
            if let Some(ref dir) = dir.as_opt_path() {
                let dir = dir.clone();
                if !self.mem_buf.is_empty() {
                    return Err(crate::Error::programming(
                        "sync() should be called before finalize_indexes()",
                    ));
                }

                let _lock = ScopedDirLock::new(&dir)?;

                let meta = Self::load_or_create_meta(&self.dir, false)?;
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

                self.dir.write_meta(&self.meta, self.open_options.fsync)?;
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
            if let Some(dir) = this.dir.clone().as_opt_path() {
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
            if let Some(ref dir) = self.dir.as_opt_path() {
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
        path: &GenericPath,
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
    fn load_or_create_meta(path: &GenericPath, create: bool) -> crate::Result<LogMetadata> {
        match path.read_meta() {
            Err(err) => {
                if err.io_error_kind() == io::ErrorKind::NotFound && create {
                    let dir = path.as_opt_path().unwrap();
                    // Create (and truncate) the primary log and indexes.
                    let primary_path = dir.join(PRIMARY_FILE);
                    let mut primary_file =
                        File::create(&primary_path).context(&primary_path, "cannot create")?;
                    primary_file
                        .write_all(PRIMARY_HEADER)
                        .context(&primary_path, "cannot write")?;
                    let _ = utils::fix_perm_file(&primary_file, false);
                    // Start from empty file and indexes.
                    let meta = LogMetadata::new_with_primary_len(PRIMARY_START_OFFSET);
                    // An empty meta file is easy to recreate. No need to use fsync.
                    path.write_meta(&meta, false)?;
                    Ok(meta)
                } else {
                    Err(err)
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
        dir: &GenericPath,
        meta: &LogMetadata,
        index_defs: &[IndexDef],
        mem_buf: &Pin<Box<Vec<u8>>>,
        reuse_indexes: Option<&Vec<Index>>,
        fsync: bool,
    ) -> crate::Result<(Arc<Mmap>, Vec<Index>)> {
        let primary_buf = match dir.as_opt_path() {
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

    /// Return the reference to the [`GenericPath`] used to crate the [`Log`].
    pub fn path(&self) -> &GenericPath {
        &self.dir
    }

    /// Load a single index.
    fn load_index(
        dir: &GenericPath,
        name: &str,
        len: u64,
        buf: Arc<dyn ReadonlyBuffer + Send + Sync>,
        fsync: bool,
    ) -> crate::Result<Index> {
        match dir.as_opt_path() {
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
        path: &GenericPath,
        buf: &'a [u8],
        offset: u64,
    ) -> crate::Result<Option<EntryResult<'a>>> {
        let data_error = |msg: String| -> crate::Error {
            match path.as_opt_path() {
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
        let path: &Path = match self.dir.as_opt_path() {
            Some(ref path) => &path,
            None => Path::new("<memory>"),
        };
        crate::Error::corruption(path, message)
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
