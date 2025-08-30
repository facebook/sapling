# Inode Storage Overview

## How Durable is Eden?

We have some guiding principles that affect the design of Eden and its
durability properties.

We intend for Eden to reliably preserve user data if the Eden processes aborts
or is killed. If the process dies, none of the user's data should be lost. Eden
crashing ought to be rare, but, especially while it's in development, it's
realistic to expect things to go wrong, including stray `killall edenfs`
commands.

However, we do not guarantee consistent data if a VM suddenly powers off or if a
disk fails. It is a substantial amount of work, and probably a performance
penalty, to be durable under those conditions.

Fortunately, thanks to commit cloud, the risk of losing days of work due to disk
or machine shutdown is low. While many engineer-hours will be spent working in
an Eden checkout, the amount of work that builds up prior to a commit is
hopefully bounded. (And perhaps someday we will automatically snapshot your
working copy!)

## Concepts

Git and Mercurial have abstract, hash-indexed tree data structures representing
a file hierarchy. (You'll find the corresponding code in `eden/fs/model`.)
Version control trees and files have a subset of the possible states that a real
filesystem can be in. For example, neither Git nor Mercurial version a file's
user or group ownership, and the only versioned permission bit is
user-executable. Also, version control systems do not support hard links.

In a non-Eden, traditional version control system, checkout operations
immediately materialize that abstract tree data structure into actual
directories and files on disk. The downside of course is that checkout becomes
O(repo) in disk operations and the entire tree is physically allocated on disk.

What makes Eden useful is that it only fetches trees and blobs from version
control as the filesystem is explored. This makes checkout O(changes). But it
raises some questions about how to expose traditional filesystem concepts like
timestamps, permission bits, and inode numbers.

## Inode States

As the filesystem is explored through FUSE, inodes are allocated to represent a
accessed source control trees and files. A given inode can then transition
between states as filesystem operations are performed on it.

### Metadata State Machine

Eden inodes transition between a series of states:

Once the parent tree has been loaded, the names, types, and ids of its children
are known. At this point, questions like "does this entry exist?" or "what is
its id?" can be answered, in addition to providing any metadata we have from the
backing version control system. (For example, Mononoke will provide file sizes
and SHA-1 hashes so Eden does not have to actually load the files and compute
them.)

To satisfy readdir() or stat() calls, however, we must give the entry an inode
number. Once an inode number has been allocated to an entry and handed out via
the filesystem, it must be remembered as long as programs can reasonably expect
them to be consistent. (e.g. for the program's lifetime or until a qualifying
"anything could happen" operation like `hg checkout`. See `#pragma once`
addendum below.)

Inode metadata such as timestamps and permission bits, once accessed, should be
remembered as long as the inode numbers are. See `make` addendum below. When
Eden forgets an inode number, the timestamps and permission bits are forgotten
too. Moreover, when the inode number is forgotten, the inode numbers of its
children must be forgotten.

There is only one type of inode metadata change that matters from the
perspective of version control: the user executable bit on files. If that bit
changes, the file and all of its parents must be marked potentially-modified.
Other metadata changes are local-only and can be ignored by version control
operations.

At the risk of repeating myself, here are some other rules. If a source control
tree entry has an inode number, its parent must also have an inode number. If an
inode is marked potentially-modified, its parent must also be marked
potentially-modified. Why? Because Eden needs to be able to crawl from the root
tree and rapidly enumerate the potentially-modified set, even at process
startup.

During a checkout operation (or otherwise) we may determine that the contents of
a file or tree now matches its unmodified state. If so, to reduce the size of
the tree Eden is tracking, it may dematerialize the tree (from the parents
down). Dematerialization must preserve inode numbers for any entries that may
currently be referenced by FUSE, but since checkout is an "anything could
happen" operation, inodes for other unmodified files could be forgotten.

For our own sanity, Eden should never hand out duplicate inode numbers.

### Data State Machine

The previous section talks about inode numbers and inode metadata (e.g.
timestamps, user, group, and mode bits).

The other half of an inode is its data: the contents of a file (or symlink) and
the entries of a tree. (Note that it's possible for an inode's data to be
modified but metadata untouched or vice versa.)

When an entry's parent is loaded, the child's name, type, and id are known, and
read operations can be satisfied by fetching a blob of data from the backing
store.

When an inode's data is modified, its state transitions to materialized.

If the modification results in a blank file (e.g. O_TRUNC), Eden doesn't even
need to wait for the blob to finish loading.

For other types of modifications, such as writes, data must be fetched from
source control before the modifications can be applied to it.

For fast status and diff operations, Eden needs to rapidly find all materialized
entries, so its parent must then be marked materialized as well (all the way to
the root).

Note that the materialized state of an inode is independent of whether it has
been modified from the contents of the file in the current source control
commit. If a non-materialized file is renamed it will still be non-materialized,
but it will be different from the current commit contents at its location.
Conversely, a file can be rewritten with contents that are identical to the
current source control state. The process of writing it will generally leave it
in a materialized state, even though it may be the same as the current source
control state at the end.

### What Does Materialized Mean?

[TODO: not sure where to put this section]

This document talks about an inode entering and leaving the 'materialized'
state. It's a bit of an unintuitive concept. If an inode is materialized, it is
potentially modified relative to its original source control object, as
indicated by its parent's entry's source control id.

Note that being materialized is orthogonal to whether a file is considered
modified or not. If a file has been overwritten with its original contents, it
will be materialized (at least temporarily) but not show up as modified from the
perspective of version control. On the other hand, if a subtree has been renamed
(imagine root/foo -> root/bar), then everything inside the subtree will not be
materialized, but will show up as modified from a status or diff operation.

If an inode is materialized, its parent must also be materialized. The
materialized status is used to rapidly determine which set of files is worth
looking at when performing a status or diff operation.

## Concrete Storage

How is all of this represented inside Eden and how do state transitions meet our
durability goals above?

### InodeMap

The InodeMap keeps track of loaded inodes and inodes that FUSE still has a
reference to.

Note that the term "loaded" is used ambiguously in Eden. When talking about
whether an inode is loaded, it means that the InodeMap has in-memory data
tracking its state. On the other hand, a FileInode can have loaded its backing
blob or not.

(TODO: should we rename InodeMap's "loaded" and "unloaded" terminology to
"known" and "remembered"?)

#### loadedInodes\_

Inode tree nodes currently loaded in memory.

- For files, that includes their ids, blob loading state, file handles into the
  overlay, timestamps, and permission bits.
- For trees, that includes tree ids, entries, timestamps.
- For both, the entry type, fuse reference count, internal reference count,
  location.
- If a child is in loadedInodes*, its parent must be in loadedInodes* too.

#### unloadedInodes\_

In-memory map from inode number to remembered inode state. When an inode is
unloaded, if it has a nonzero FUSE reference count, it is registered into this
table, which contains:

- its FUSE refcount
- its id (if not materialized)
- its permission bits
- parent inode number and child name (if not unlinked)

If a child is in unloadedInodes*, its parent must be in unloadedInodes* too.

An inode cannot be in both loadedInodes* and unloadedInodes* at the same time.

If an inode has a nonzero FUSE reference count, it should exist in either
loadedInodes* or unloadedInodes*.

#### Overlay

The Overlay is an on-disk map from inode number to its timestamps plus the
file's or tree's contents.

If a tree's child entry does not have an id (that is, it's marked as
materialized), then data for that inode must be in the overlay. Because of this
invariant, we must write the child's overlay data prior to setting it
materialized in the parent. When dematerializing, we must mark the child as
dematerialized in the parent before deleting the child's overlay data, in case
the process crashes in between those two operations.

### InodeMap State Transitions

[This section may be incomplete.]

Unknown ⟶ Loading:

- (First, load parent.)
- If parent has this entry marked materialized, load child from overlay and
  immediately transition to loaded. Otherwise...
- Insert entry in unloadedInodes\_
- Begin fetching object from ObjectStore

Loading ⟶ Loaded:

- If this is a tree, when the load completes, check the overlay.
  - The overlay might have some remembered inode numbers.
  - TODO: if eden crashed while materializing up a tree, that state needs to be
    corrected or dropped here.
- Construct Inode type
- Remove from unloadedInodes* and insert into loadedInodes*

Loaded ⟶ Unloaded:

- If the mount is being unmounted
  - If unlinked, remove it from the overlay (it can never be accessed again)
  - Otherwise, update metadata in Overlay
- Otherwise (we probably need to remember the inode number)
  - If unlinked, remove it from the overlay
  - Otherwise,
    - If fuseCount is nonzero, insert inode in unloadedInodes\_
    - If inode is a tree and any of its children are in unloadedInodes*, insert
      inode in unloadedInodes*
    - Otherwise... forget everything about the inode.

### TreeInode State Machine

TreeInode can only make two state transitions:

Unmaterialized ⟶ Materialized:

- When a tree is modified, it is marked materialized (recursively up the tree)
- Its contents are written to the Overlay

Materialized ⟶ Unmaterialized:

- When Eden notices the entries match the backing source control Tree, and it
  has no materialized children, it is marked dematerialized.
- Note that the Tree's parent must be updated prior to removing the child's
  overlay data.

### FileInode State Machine

FileInode's transitions are relatively isolated and uninteresting. See the
comments in FileInode.h for details, but I'll enumerate the currently legal
transitions here.

- not loaded ⟶ loading
- not loaded ⟶ materialized (O_TRUNC)
- loading ⟶ loaded
- loading ⟶ materialized (O_TRUNC)
- loaded ⟶ materialized

[TODO: dematerialization]

## Addenda

### atime

It is very hard and probably not useful for Eden to try to accurately maintain
last-access times for files. In fact, FUSE does not really try:

https://sourceforge.net/p/fuse/mailman/message/34448996/

### #pragma once

On a previous version of Eden, I saw some intermittent build failures that
looked like this:

```
rocksdb/src/db/memtable_list.h:40:7: error: redefinition of 'class rocksdb::MemTableListVersion'
rocksdb/src/db/memtable_list.h:40:7: error: previous definition of 'class rocksdb::MemTableListVersion'
```

The issue was that Eden would occasionally allocate a new inode number for a
nonmaterialized file, and `#pragma once` relies on consistent inode numbers to
avoid including the same file twice. Previously, we had some open questions
about whether Eden really did need to provide 100% consistent inode numbers for
nonmaterialized files, but it seems the answer is yes, at least while the mount
is up (including graceful takeover).

### make

Make uses the filesystem to remember whether to rebuild a target. It does so by
comparing the mtime of the target with its dependencies. If the target is newer
than all dependency, it is not rebuilt.

For Eden to avoid spurious rebuilds with make projects, it must strive to
remember mtimes allocated to unmodified files (and thus presumably the
unmodified file's inode number). If checking out from unmodified tree A to tree
B forgets that directory's inode numbers and the inode numbers of its children,
the mtimes allocated to the source files could appear to advance, causing
spurious builds.
