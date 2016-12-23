Inode-related Locks
-------------------

## InodeBase's `location_` lock:

No other locks should be acquired while holding this lock.
Two `location_` locks should never be held at the same time.

This field cannot be updated without holding both the EdenMount's rename lock
and the `location_` lock for the InodeBase in question.

Note that `InodeBase::getLogPath()` acquires `location_` locks.  This function
is used in log statements in many places, including in places where other locks
are held.  It is therefore important to ensure that the `location_` lock
remains at the very bottom of our lock-ordering stack.

## InodeMap `data_` lock:

No other locks should be acquired while holding this lock, apart from InodeBase
`location_` locks.  (InodeBase `location_` locks are only held with the
InodeMap lock already held for the purpose of calling `inode->getLogPath()` in
VLOG statements.)

In general it should only be held very briefly while doing lookups/inserts on
the map data structures.  Once we need to load an Inode the InodeMap lock is
released for the duration of the load operation itself.  It is re-acquired when
the load completes so we can insert the new Inode into the map.

## FileInode lock:

The InodeBase `location_` lock may be acquired while holding a FileInode's
lock.

## TreeInode `contents_` lock:

- The InodeMap lock may be acquired while holding a TreeInode's `contents_`
  lock.

- The InodeBase `location_` lock may be acquired while holding a TreeInode's
  `contents_` lock.

- A FileInode's lock may be acquired while holding its parent TreeInode's
  `contents_` lock.

- In some situations the same thread acquires multiple `contents_` locks
  together.  These must be acquired in the following order:
  - If TreeInode A is a parent or ancestor of TreeInode B, A's `contents_` lock
    must be acquired first.
  - If TreeInodes A and B are not in a parent/child relationship, acquire A's
    `contents_` lock first if and only if A's address is lower than B's

## EdenMount's rename lock:

This lock is a very high level lock in our lock ordering stack--it is typically
acquired before any other locks.

This lock is held for the duration of a rename or unlink operation.  No
InodeBase `location_` fields may be updated without holding this lock.
