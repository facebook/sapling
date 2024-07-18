A few notes about renames:

# Rename Lock

There is a mountpoint-wide rename lock that is held during any rename or unlink
operation. An Inode's path cannot be changed without holding this lock.

However, we currently do not hold the rename lock when creating new files or
directories. Therefore TreeEntry `contents_.entries` fields may change even when
the rename lock is not held. (We could potentially revisit this choice later and
require holding the rename lock even when creating new inodes.)

# Renaming over directories

Rename supports renaming one directory over an existing directory, as long as
the destination directory is empty. This means we must (a) be able to safely
check if the directory is currently empty, and (b) be able to prevent new files
or directories from being created inside the destination directory once the
rename has started.

We currently achieve this by acquiring the destination directory's `contents_`
lock. This does mean that a rename operation may hold up to 3 TreeInode locks
concurrently: the source directory, the destination parent directory, and the
destination child directory. The [InodeLocks](InodeLocks.md) document describes
the lock ordering requirements for acquiring these 3 locks.

This also means that create() and mkdir() operations must check if the parent
directory is unlinked _after_ acquiring the parent directory's contents lock.

# Handling unloaded children

When rename() (and unlink()/rmdir()) is invoked, the parent directories have
already been loaded (typically having been identified via inode number).
However, the affected children may not have been loaded yet, and are referred to
by name.

We had a few choices for how to deal with this situation.

For now we have opted to always load the child entries in question before
performing the rename. This is slightly tricky, as loading the child may take
some time, and another rename or unlink operation may also be in progress, and
may affect the child in question before our operation can take place. One option
would have been to hold the rename lock while waiting on the children to be
loaded. However, this would have blocked all other rename/unlink/rmdir
operations for the duration of the load, which seems undesirable. Instead, we
wait for the load to complete, then double check to confirm that the named entry
that we desire is actually loaded. The original inode we loaded may have been
renamed or unlinked, so we may find an unloaded entry or no entry at all. If we
find an unloaded entry we have to repeat the load operation. We therefore may
have to retry loading the requested children multiple times before we can make
progress, but we should eventually succeed or fail. Once the children are loaded
the rename itself is then fairly straightforward.

Another option would have been to allow the rename even though the requested
children are not loaded. The main downside with this approach is that we still
need to confirm if the destination child is an empty directory or not. This
would have meant either loading the destination child inode anyway, or storing
some extra data to track if an unloaded inode is an empty directory or not. This
also makes the inode loading code more complicated, as an inode may be unlinked
or renamed while it is already in the process of being loaded. When the load
completes we would need to double-check which parent TreeInode the new entry
needs to be inserted into. All-in-all this felt more complicated than simply
always loading the affected children before performing rename/unlink/rmdir
operations.
