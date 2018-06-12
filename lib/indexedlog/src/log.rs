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
