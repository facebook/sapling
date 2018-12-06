Caching in Eden
===============

[This captures the state of Eden as of November, 2018. The information below may
change.]

To achieve good performance, Eden relies on caching at multiple layers.

## FUSE

Requests come into Eden through either FUSE or Thrift. Most requests from FUSE
are automatically cached by the kernel's VFS cache. This is especially important
for Eden, since any request that makes it into our user-space FUSE daemon will
inherently require multiple context switches.

The latency of a cached operation is somewhere below a microsecond. On the other
hand, any FUSE request that actually makes it into our FUSE daemon will take 10+
microseconds.

When returning a successful FUSE response to the kernel, Eden returns an
infinite expiry. Because Eden tracks all writes to the working copy, it can
explicitly invalidate any inodes the kernel may have cached.

## Blobs

A blob is an in-memory copy of the content of a file committed to source
control. Blobs are loaded over the network or from packs on disk.

Eden maintains a capped-size, minimum-entry, in-memory LRU cache for
recently-accessed blobs. It is common for a blob to be read multiple times in
quick succession, and reloading the blob each time would be inefficient.

The design of this cache attempts to satisfy competing objectives:

* Minimize blob reloads under Eden's various access patterns
* Fit in a mostly-capped memory budget
* Avoid performance cliffs under pathological access patterns
* Maximize memory available to the kernel's own caches, since they have the
  highest leverage.

The cache has a maximum size (default 40 MiB as of this writing), and blobs are
evicted when the size is exceeded. However, the cache maintains a minimum entry
count (default 16 as of this writing) to avoid excessive reloads in the
situation that many large files are opened and read in sequence. (Consider the
example of some large databases or textures whose pages are read semi-randomly.)

Of course, both of these settings are configurable and worth some
experimentation.

One interesting aspect of the blob cache is that Eden has a sense of whether a
request is likely to occur again. For example, if the kernel does not support
caching readlink calls over FUSE, then any symlink blob should be kept in Eden's
cache until evicted. If the kernel *does* cache readlink, then the blob can be
released as soon it's been read, making room for other blobs.

A more complicated example is that of a series of reads across a large file.
Assume a program wants to read the entirety of a large file. Eden will receive a
sequence of read requests for increasing ranges. Eden tracks each read in a
CoverageSet data structure, and only once the CoverageSet encompasses the entire
blob, Eden evicts the blob from its cache.

Blobs are evicted from cache when:

* The blob cache is full and exceeds its minimum entry count.
* The blob has been read by the kernel and the kernel cache is populated.
* A file inode is materialized and future requests will be satisfied by the
  overlay.
* The kernel has evicted an inode from its own inode cache after reading some of
  the blob.

## Blob Metadata

One of the most common requests in Eden is computing a file's SHA-1 hash. Buck
requests SHA-1 hashes of all sources to compute rule keys to decide whether to
rebuild a target or read it from cache. Watchman will also request SHA-1 hashes
from Eden if requested by its subscribers.

There's a less obvious reason Eden looks up SHA-1s too. In Mercurial, blobs have
identifying hashes. But because Mercurial stores metadata about renames and
moves within the blob objects themselves, the blob hash can change without its
contents having changed. For the purpose of computing diff and status, Eden
needs to know if two blobs have the same contents when their blob IDs differ.

To make these operations efficient, Eden maintains an in-memory, million-entry
Blob ID => (64-bit size, 20-byte SHA-1) LRU cache. One million entries fits in
under 100 MB and can hold the SHA-1s of all files in most large repositories.

## Local Store

To avoid needing to fetch blobs and trees repeatedly from the network, Eden
stores the objects in its own local store (currently RocksDB).

When a blob is fetched from the network, Eden assumes now is a good time to
compute the content hash. If the upstream (e.g. Mononoke) already knows a blob's
SHA-1 and size, Eden does not recompute it. Afterwards, the content hash and
size are stored both in the in-memory cache and the local store.

Local store eviction is an unsolved problem. The `eden gc` command flushes
everything that is known to be recreatable, but otherwise the local store will
grow without bounds.

## Mercurial Caches

Mercurial has its own blob and tree caches. Eden will import data from them when
possible. It currently mirrors that data into its own local store but that's an
easy opportunity to remove unnecessary disk consumption and IO.
