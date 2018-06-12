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
