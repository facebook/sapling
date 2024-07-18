# EdenFS Glossary

### Backing Repository

The backing repository is the local, on-disk, source control
[repository](#repository) from which EdenFS fetches source control data for a
[checkout](#checkout).

When fetching data from Mercurial or Git, EdenFS requires a separate, local,
bare repository to be kept somewhere for EdenFS to use to fetch source control
data. This bare repository is the backing repository. Multiple checkouts can
share the same backing repository.

### Backing Store

The term backing store is sometimes used to refer to the underlying data source
used to fetch source control object information. This term comes from the
[`BackingStore`](../store/BackingStore.h) class which provides an API for
fetching data from source control. This is an abstract API which can, in theory,
support multiple different source control types, such as EdenSCM, Mercurial, or
Git.

Fetching data from the backing store is generally expected to be an expensive
option which may end up fetching data from a remote host.

The backing store generally refers to EdenFS's internal implementation for
fetching source control data. The [backing repository](#backing-repository) is
the concrete, local, on-disk storage of the underlying source control state.

### Checkout

When we use the term "checkout" in EdenFS we mean a local client-side source
control checkout, particularly the working directory state.

We use this in contrast with the term ["repository"](#repository), which we
generally use to refer to the source control metadata storage. The source
control repository stores information about historical commits and objects,
whereas the checkout displays the current working directory state for the
currently checked-out commit.

EdenFS exposes checkouts to users; it fetches underlying source control data
from a repository.

Our usage of this terminology has evolved somewhat over the course of EdenFS's
development. Early in development we also used the terms "client" and "mount
point" to refer to checkouts. In a handful of locations you may still see
references to these older terms (in particular, the `EdenMount` class), but for
most new code and documentation we have attempted to be consistent with the use
of the term "checkout".

### Inode

An inode represents a file or directory in the filesystem. This terminology is
common to Unix filesystems. The
[inode wikipedia entry](https://en.wikipedia.org/wiki/Inode) has a more complete
description.

### Journal

The Journal is the data structure that EdenFS uses to record recent modifying
filesystem I/O operations. This is used to implement APIs like
`getFilesChangedSince()`, which is in turn used by watchman to tell clients
about recent filesystem changes.

### Loaded / Unloaded

The terms "loaded" and "unloaded" are used to refer to whether EdenFS has state
for a particular inode loaded in memory or not. If EdenFS has a `FileInode` or
`TreeInode` object in memory for a particular file or directory, then that file
is referred to as loaded. Otherwise, that file or directory is considered
unloaded.

By default, when a checkout is first mounted, most inodes are unloaded. EdenFS
then lazily loads inodes on-demand as they are accessed.

### Local Store

The local store refers to EdenFS's local cache of source control data. This data
is stored in the [EdenFS state directory](#state-directory) at `.eden/storage`.

Over time EdenFS has been moving away from tracking data in the local store,
instead relying more on the underlying source control data fetching mechanisms
to cache things in source control specific data structures when appropriate.

### Materialized / Non-Materialized

Inodes are considered materialized if we do not know a source control object ID
that can be used to look up the file or directory contents. Non-materialized
inodes have contents identical to a source control object.

When a checkout is first cloned, all inodes are non-materialized, as we know
that the root directory corresponds to the root source control tree for the
current commit. Each of its children correspond to its corresponding children in
source control, so they are also non-materialized.

When a file is modified from its source control state, it becomes materialized.
This is because we can no longer fetch the file contents from source control.
Following this logic, brand new files that are created locally immediately start
in the materialized state. Also, if a file no longer corresponds to a known
source control object, the parent directory also no longer corresponds to a
known source control tree. This means that when a child inode is materialized,
its ancestors are also materialized recursively upwards until the root of the
repo or an already materialized tree is reached.

Materialized files are stored in the [overlay](#overlay). Non-materialized files
do not need to be stored in the overlay, as their contents can always be fetched
from the source control repository.

For more details see the
[Inode Materialization](Inodes.md#inode-materialization) documentation.

In ProjFS mounts, there is an additional special case for materialized files.
Files that have been renamed are considered materialized inodes. Technically, we
still know the source control object associated with the inode, however, we no
longer store this association in the overlay. ProjFS always will make a read
request for these files with the original path and reads are only served from
source control objects in ProjFS.

### Populated

Inodes or files are considered "populated" when their contents have been
observed by the kernel, but the file has not yet been modified. For ProjFS
mounts "populated" this means the contents are present on the filesystem and
reads are going to be directly handled by ProjFS until we invalidate the file.
In FUSE and NFS mounts this means the kernel may have the file contents in its
caches, though FUSE or NFS may have decided to evict them from cache of its own
will.

For ProjFS mounts, "populated" is roughly equivalent to the hydrated placeholder
state for files. For directories "populated" is roughly equivalent to
placeholders without materialized children.

No populated files and directories correspond to materialized inodes and vice
versa. These states are intentionally independent. i.e. populated is defined as
files the kernel knows about - materialized ones.

### Overlay

The overlay is where EdenFS stores information about
[materialized](#materialized--non-materialized) files and directories.

Each checkout has its own separate overlay storage. This data is stored in the
[EdenFS state directory](#state-directory) at
`.eden/clients/CHECKOUT_NAME/local`

The term "overlay" comes from the fact that it behaves like an
[overlay filesystem](https://en.wikipedia.org/wiki/Union_mount) (also known as a
union filesystem), where local modifications are overlayed on top of the
underlying source control state.

### Repository

The term "repository" is used to refer to the source control system's storage of
source control commit, directory, and file data.

Contrast this to the term [checkout](#checkout) above, which refers specifically
to the working directory state.

### State Directory

The state directory is where EdenFS stores all of its local state. The default
location of this directory can be controlled in the system configuration
(`/etc/eden/edenfs.rc`) or the user-specific configuration (`$HOME/.edenrc`),
but it generally defaults to `$HOME/.eden/`. However, it defaults to
`$HOME/local/.eden` in some Meta environments.
