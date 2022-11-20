# IndexedLog

IndexedLog is the main on-disk storage format used by various components in
Sapling.

## Background

Historically, [revlog](https://www.mercurial-scm.org/wiki/Revlog) was the main
storage format, but it has a few limitations:

- **Revisions have to be topologically ordered.** When revisions are fetched on
  demand, if a later revision was fetched first, then it's impossible to append
  an older (ancestor) revision.
- **Lookup by hash can trigger a linear scan.** The slowness this causes
  becomes noticeable when there are lots of revisions. It can be worked around
  by building separate indexes.
- **Not general purpose enough to fit various use-cases.** For example, the
  [obsstore](https://www.mercurial-scm.org/wiki/CEDObsstoreFormat) would
  require looking up a record by various indexes: predecessors or successors,
  and one record can have multiple predecessors or successors. In Revlog, one
  record can only have one SHA1 hash as its key.

[Git](https://git-scm.com)'s format (loose and pack files) does not have the
topological order limitation, and lookup by hash is ideally O(log N), unless
there are too many pack files. But it requires periodical `repack` to maintain
performance, and does not support the multi-index use-case.

[SQLite](https://www.sqlite.org/) is a powerful library that satisfies the
multi-index use-cases, and can maintain time complexity without "repack".
However, the main problem is that historically we use the "append-only"
strategy to achieve lock-free reads, but SQLite requires locking for both read
and write.

IndexedLog is built to achieve the following desired properties:

- **O(log N) lookup** and does not require `repack` to maintain performance.
- **Insertion by hash** without topological order limitation.
- **Lock-free reading** which primarily means "append-only larger files" and
  "atomic-replace small files" using the filesystem APIs we have today.
  Transactional filesystem or chunk-level copy-on-write could make a
  difference but they are generally not available.
- **General purpose** with multi-index and multi-key per entry support.

In addition, we think the property below is nice to have:

- **Data integrity.** In case of hard reboots, or accidental `sed -i` on the
  repository data, we want to understand exactly what parts of the data are
  corrupted, and have a way to recover.

## Log

An IndexedLog consists of a Log and multiple surrounding Indexes. The Log is
the single source of truth for the data. The indexes are derived purely from
the Log and index functions. Corrupted indexes can be deleted and rebuilt as
long as the Log is fine.

A Log stores entries in insertion order. An entry consists of a slice of
bytes. `Log` is interface-wise similar to `LinkedList<Vec<u8>>`, but only
accepts `push_back`, not `push_front`.

A Log supports the following operations:
- Iterate through all entries in insertion order.
- Append a new entry to the end.

A Log does not support:
- Read an entry by its offset (random access).
- Remove an entry.

Unlike a relational or document database, Log itself does not define the
meaning of the bytes in an entry. The meaning is up to the application to
decide.

## Index

A Log can have multiple indexes. An index has a name, and a pure function that
takes the byte slice of an entry, and outputs a list of `IndexOutput`s.

Each `IndexOutput` is an enum that instructs IndexedLog to do one of the
following:
- Insert a key (in bytes) that points to the current entry.
- Remove a key or remove keys with a given prefix.

Unlike databases, the index function is in native Rust and not a separate
language like SQL or JSON. Note that it is impossible to serialize a compiled
Rust function to disk, so applications need to provide the exact same index
functions when loading an IndexedLog from disk.

If you change an index function, you need to also change the index name to
prevent the IndexedLog from picking up the wrong index data.


## Standalone index

The Index can be used independently from Log. Its interface is similar
to `BTreeMap<Vec<u8>, LinkedList<u64>>`, but uses the filesystem instead of
memory for the main storage.

An Index supports:
- Insert (key, value). The value is inserted at the front of the existing
  linked list. Alternatively, the existing linked list can be dropped.
- Lookup keys by a range.
- Delete keys in a range.

Internally, the key portion of the index is a
[radix tree](https://en.wikipedia.org/wiki/Radix_tree) where a node has 16
children (4 bits). This is used to support hex prefix lookup. The value portion
is a singly-linked list which supports `push_front`, but not `push_back`.

The on-disk format uses [persistent data structure](https://en.wikipedia.org/wiki/Persistent_data_structure#Trees)
to achieve lock-free reading. The main index is append-only. The pointer to the
root tree node is a small piece of data tracked separately using
atomic-replace.

When used together with a Log, the `u64` part of `LinkedList<u64>` is used as
file offsets. The offsets are not exposed in public APIs of Log to avoid
misuse. The Log allows the on-disk indexes to lag for some entries because
updating the index for 1 entry takes O(log N) space, inefficient for frequent
small writes. The lagging portion of index will be built on demand in memory.

## Concurrent writes

When an IndexedLog (or a standalone Index) gets loaded from disk, it is like
taking a snapshot. Changes on disk afterwards won't affect the already loaded
IndexedLog (as long as all writes to the files go through indexedlog APIs).

Writes are buffered in memory, lock-free. They are invisible to other processes
or other already loaded IndexedLogs.

A `sync` operation is needed to write data to disk, or load changed data from
disk. The `sync` will take a filesystem lock to prevent other writers, pick up
the latest filesystem state if anything has changed on disk, write the updated
log and indexes to disk, then release the lock.

If 2 processes (or 2 IndexedLogs in one process) are `sync()`-ing to the same
IndexedLog on disk concurrently, both their pending changes will be written.
The order of the written data is unspecified, depends on which one obtains the
filesystem lock first.

## Data integrity

Both Log and Index use [xxhash](http://www.xxhash.com/) for data integrity.
Log writes a XXH32 or XXH64 checksum per entry depending on the size of the
entry.  Index internally maintains checksum entries per 1MB data. All data
reading triggers integrity checks. Errors will be reported to the application.

IndexedLog supports a "repair" operation which truncates the Log to entries
that pass the integrity check and then rebuilds the corrupted or outdated
indexes.

## RotateLog

RotateLog applies the
[log rotation](https://en.wikipedia.org/wiki/Log_rotation) idea to IndexedLog.
RotateLog maintains a list of Logs. When a Log exceeds certain size limit,
RotateLog creates a new Log and optionally delete the oldest ones.

RotateLog is intended to be used for client-side caching, where the client
wants space usage to be bounded, and the data can be re-fetched from the
server.
