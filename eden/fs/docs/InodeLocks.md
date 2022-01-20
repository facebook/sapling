Inode-related Locks
-------------------

## InodeBase's `location_` Lock:

No other locks should be acquired while holding this lock.
Two `location_` locks should never be held at the same time.

This field cannot be updated without holding both the EdenMount's rename lock
and the `location_` lock for the InodeBase in question.

Note that `InodeBase::getLogPath()` acquires `location_` locks.  This function
is used in log statements in many places, including in places where other locks
are held.  It is therefore important to ensure that the `location_` lock
remains at the very bottom of our lock-ordering stack.

## InodeMap `data_` Lock:

No other locks should be acquired while holding this lock, apart from InodeBase
`location_` locks (InodeBase `location_` locks are only held with the
InodeMap lock already held for the purpose of calling `inode->getLogPath()` in
logging statements).

In general, it should only be held very briefly while doing lookups/inserts on
the map data structures.  Once we need to load an Inode, the InodeMap lock is
released for the duration of the load operation itself.  It is re-acquired when
the load completes so we can insert the new Inode into the map.

## InodeMetadataTable `state_` Lock:

No other locks should be acquired while holding this lock.

In general, it should only be held very briefly while doing lookups/inserts on
InodeTable's index data structures.

## FileInode Lock:

The InodeBase `location_` lock may be acquired while holding a FileInode's
lock.

## TreeInode `contents_` Lock:

- The InodeMap lock may be acquired while holding a TreeInode's `contents_`
  lock.

- The InodeBase `location_` lock may be acquired while holding a TreeInode's
  `contents_` lock.

- A FileInode's lock may be acquired while holding its parent TreeInode's
  `contents_` lock.

In some situations, the same thread acquires multiple `contents_` locks
together.

  - Some code paths hold a parent TreeInode's `contents_` lock while accessing
    its children, and then acquires a child TreeInode's `contents_` lock while
    still holding the parent TreeInode's lock.

  - The `rename()` code may hold up to 3 TreeInode locks.  It always holds the
    `contents_` lock on both the source TreeInode and the destination
    TreeInode.  Additionally, if the destination name refers to an existing
    TreeInode, the rename() holds its `contents_` lock as well, to ensure that
    it is empty, and to prevent new entries from being created inside this
    directory once the rename starts.

To prevent deadlocks, the lock ordering constraints for TreeInode `contents_`
are as follows:

- If you are not holding the mountpoint rename lock, you can only acquire
  a TreeInode `contents_` lock if the other `contents_` locks you are holding
  are for this TreeInode's immediate parents (e.g., if you are already
  holding another `contents_` lock, it must be for this TreeInode's parent.  If
  you are holding two other `contents_` locks, it must be for this TreeInode's
  parent and grandparent).

  Note, however, that acquiring multiple TreeInode contents locks is discouraged.
  When possible, it is preferred to release the lock on the parent TreeInode
  before locking the child.  Acquiring locks on more than 2 levels of the tree
  hierarchy is technically safe from a lock ordering perspective, but is also
  strongly discouraged.

- If you are holding the mountpoint rename lock, it is safe to acquire multiple
  TreeInode locks at a time.  However, if there is an ancestor/child
  relationship between any of the TreeInodes, the ancestor lock must be
  acquired first.  This avoids lock ordering issues with other threads that are
  not holding the rename lock.  Among unrelated TreeInodes, no particular
  ordering is required.

## EdenMount's Rename Lock:

This lock is a very high level lock in our lock ordering stack--it is
acquired before any other individual inode-specific locks.

This lock is held for the duration of a rename or unlink operation.  No
InodeBase `location_` fields may be updated without holding this lock.

## EdenMount's Current Snapshot Lock:

This lock is a leaf lock that is held for short duration at the beginning and
end of a checkout operation. Once acquired during checkout, the
checkoutInProgress flag is set and the lock is released.

On Windows, this lock may need to be taken by recursive ProjectedFS callbacks
which will need to read the current snapshot to walk the Tree hierarchy.
