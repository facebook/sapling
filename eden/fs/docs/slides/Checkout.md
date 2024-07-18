---
marp: true
---

# Checkout

As of 08/01/2023

---

# Checkout modes

- `DRY_RUN`: Ran first on `hg update`, merely reports conflicts to Mercurial
- `NORMAL`: Ran after the first `DRY_RUN`, does the actual update
- `FORCE`: Ran on `hg update -C`, this will always prefer the destination commit
  file content on conflict.

---

# Gist of the algorithm

- Only do the minimum required to sync the working copy to the destination
  - This means not recursing down to directories that the OS isn't aware of.
  - But also not recursing down to directories that are identical between the
    working copy and the destination commit.
- For every files/directories whose content needs to be updated, EdenFS will
  notify the OS to invalidate the file/directory.

---

# Invalidation

- This is done via:
  - `invalidateChannelEntryCache`: informs the OS that the given filename in the
    directory has changed. This also needs to be called for new files to make
    sure the OS discards its negative path cache.
  - `invalidateChannelDirCache`: informs the OS that the directory content has
    changed. In particular, if a file is added or removed from the directory,
    this needs to be called.

---

## Invalidation on ProjectedFS

- `invalidateChannelEntryCache`: calls into `PrjDeleteFile` to remove the
  placeholder/full file from disk. Future requests to this file will have EdenFS
  re-create the file on disk.
- `invalidateChannelDirCache`: calls into `PrjMarkDirectoryAsPlaceholder`, this
  forces directory listing to always be served by EdenFS for that directory.
  This is always called after a directory is fully processed.

---

## Invalidation on FUSE

FUSE was the original FsChannel of EdenFS and thus the invalidation were built
around the FUSE semantics and the EdenFS invalidation function map 1:1 to FUSE
invalidation opcodes.

Of note is that invalidations are sent asynchronously.

- `invalidateChannelEntryCache`: sends `FUSE_NOTIFY_INVAL_ENTRY` to the kernel
- `invalidateChannelDirCache`: sends `FUSE_NOTIFY_INVAL_INODE` to the kernel

---

## Invalidation on NFS

On NFS, there are no native mechanism to tell the kernel to invalidate its
cache. Instead, EdenFS rely on NFS clients looking at the `mtime` of
files/directories to invalidate its caches.

Similarly to FUSE, NFS invalidations are sent asynchronously.

- `invalidateChannelEntryCache`: Does nothing, the code rely on the parent
  directory being invalidated with an updated `mtime` which flushes caches.
- `invalidateChannelDirCache`: Uses `chmod(mode)` to force a no-op `SETATTR` to
  be sent to EdenFS. Historically, EdenFS was merely opening and closing the
  file to take advantage of the "close to open" consistency, but macOS doesn't
  respect it.

---

# Core Checkout

- `TreeInode::checkout`: entry point for a directory. It spawns `CheckoutAction`
  by comparing the currently checked out `Tree` to the destination `Tree` in
  `TreeInode::computeCheckoutActions`. Once all actions have completed,
  invalidation is run for that directory and the overlay is updated.
- `TreeInode::processCheckoutEntry`: called by `TreeInode::checkout` and runs
  with the `contents_` lock held, this will handle addition and removal
  immediately and defers conflict checks to `CheckoutAction`.
- `CheckoutAction`: wrapper class to simplify loading `Blob` sha1 and `Tree`.
  Once loaded and conflicts are resolved, calls
  `TreeInode::checkoutUpdateEntry`.
- `TreeInode::checkoutUpdateEntry`: called once the `Blob` and `Tree` are
  loaded, this will take the `contents_` locks, revalidate it to ensure that no
  new conflicts arose since `TreeInode::checkout` released the lock, and perform
  in place modification. For directories, this recurses down by calling
  `TreeInode::checkout`.

---

# Overlay update

- At the end of `TreeInode::checkout`, after processing all the
  `CheckoutAction`, the overlay is updated to the destination state
  (`TreeInode::saveOverlayPostCheckout`).
- Since the overlay for this `TreeInode` is updated, this needs to be recorded
  in the parent directory overlay, which will force it to be materialized and
  written to disk (potentially recursively).
  - The number of overlay writes for a directory is thus potentially
    `O(number of subdirectory)`

---

# Q&A
