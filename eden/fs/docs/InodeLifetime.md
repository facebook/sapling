Inode Ownership
===============

Inodes are managed via `InodePtr` objects.  `InodePtr` is a smart-pointer class
that maintains a reference count on the underlying `InodeBase` object, similar
to `std::shared_ptr`.

However, unlike `std::shared_ptr`, inodes are not necessarily deleted
immediately when their reference count drops to zero.  Instead, they may remain
in memory for a while in case they are used again soon.

Owners
------

- `InodeMap` holds a reference to the root inode.  This ensures that the root
  inode remains in existence for as long as the `EdenMount` exists.

- Each inode holds a reference to its parent `TreeInode`.  This ensures that
  if an inode exists, all of its parents all the way to the mount point root
  also exist.

- For all other call sites, callers obtain a reference to an inode when they
  look it up.  The lookup functions return `InodePtr` objects that the call
  site should retain for as long as they need access to the inode.

Non-Owners
----------

- `InodeMap` does not hold a reference to the inodes it contains.  Otherwise,
  it would never be possible to unload or destroy any inodes.  Instead, the
  `InodeMap` holds raw pointers to inode objects.  When `Inode` objects are
  unloaded they are always explicitly removed from the `InodeMap`'s list of
  loaded inodes.

- A `TreeInode` does not hold a reference to any of its children.  Otherwise,
  this would cause circular reference, since each child holds a reference to
  its parent `TreeInode`.  The `TreeInode` is always explicitly informed when
  one of its children inodes is unloaded, so it can remove the raw pointer to
  the child from its child entries map.


Inode Lookup
============

Inodes may be looked up in one of two ways, either by name or by inode number.
`TreeInode::getOrLoadChild()` is the API for doing inode lookups by name,
and `InodeMap::lookupinode()` is the API for doing inode lookups by inode
number.

Either of these two APIs may have to create the inode object.  Alternatively,
if the specified inode already exists, they will increment the reference count
to the existing object and return it.  It is possible the inode is already
present in the `InodeMap`, but was previously unreferenced, so these APIs may
increment the reference count from 0 to 1.

Simultaneous Lookups
--------------------

The `InodeMap` class keeps track of all currently loaded inodes as well as
information about inodes that have inode numbers allocated but are not loaded.
For each unloaded inode, `InodeMap` records if it is currently being loaded.
This allows `InodeMap` to avoid starting two load attempts for the same inode.
If a second lookup attempt occurs for an inode already being loaded, `InodeMap`
handles notifying both waiting callers when the single load attempt completes.


Inode Unloading
===============

Inode unloading can be triggered by several events:

## Inode reference count going to zero

When the inode reference count drops to zero, we have a chance to decide if we
want to unload the inode or not.

When shutting down the mount point, we always destroy each inode as soon as its
reference count goes to zero.

If the inode is unlinked and its FUSE reference count is also zero, we also
destroy the inode immediately.

In other cases we generally leave the inode object loaded, but it would be
valid to decide to unload it based on other criteria (for instance, we could
decide to immediately unload unreferenced inodes if we are low on memory).

## FUSE reference count going to zero

When the FUSE reference count goes to zero, we should destroy the inode
immediately if it is unlinked and its pointer reference count is also zero.

To simplify synchronization, we currently collapse this case into the one
above: we only decrement the FUSE reference count on a loaded inode when we are
holding a normal `InodePtr` reference to the inode.  Therefore, we will always
see the normal reference count drop to zero at some point after the FUSE
reference count drops to zero, and we process the unload at that time.

## On demand

We will likely add a periodic background task to unload unreferenced inodes
that have not been accessed in some time.  This unload operation could also be
triggered in response to other events (for instance, a thrift call, or going
over some memory usage limit).

Synchronization and the Acquire Count
-------------------------------------

Synchronization of inode loading and unloading is slightly tricky, particularly
for unloading.

### Loading

When loading an inode, we always hold the `InodeMap` lock to check if the inode
in question is already loaded or if a load is in progress.  Once the inode is
loaded, we acquire its parent `TreeInode`'s `contents_` lock, then the
`InodeMap` lock (in that order), so we can insert the inode into it's parent's
entry list and into the `InodeMap`'s list of loaded inodes.

### Updating Reference Counts

`InodePtr` itself does not hold any extra locks when performing reference
count updates.  The main inode reference count is updated with atomic
operations, but without any other locks held.

However, there is one important item to note here: updates done via `InodePtr`
copying can never increment the reference count from 0 to 1.  The lookup APIs
(`TreeInode::getOrLoadChild()` and `InodeMap::lookupInode()`) are the only two
places that can ever increment the reference count from 0 to 1.  Both of these
lookup APIs hold a lock when potentially updating the reference count from 0 to
1.

`TreeInode::getOrLoadChild()` holds the parent `TreeInode`'s `contents_` lock,
and `InodeMap::lookupInode()` holds the `InodeMap` lock.  This means that if
you hold both of these locks and you see that an inode's reference count is
currently 0, no other thread can acquire a reference count to that inode.

### Preventing Multiple Unload Attempts

Holding the parent `TreeInode`'s `contents_` lock and the `InodeMap` lock
ensures that no other thread can acquire a new reference on an inode, but that
alone does not mean it is safe to destroy the inode.  We still need to prevent
multiple threads from both trying to destroy an inode.

For instance, consider if thread A destroys the last `InodePtr` to an inode,
dropping its reference count to 0.  However, before thread A has a chance to
grab the `TreeInode` and `InodeMap` locks and decide if it wants to unload the
inode, thread B looks up the inode, increasing the reference count from 0 to 1,
but then immediately destroys its `InodePtr`, dropping the reference count back
to 0.

In this situation thread A and thread B have both just dropped the reference
count to 0.  We need to make sure that only one of these two threads can try to
destroy the inode.

This is achieved through another counter, called the "acquire" counter.
This counter is incremented each time the inode reference count goes from 0 to
1, and decremented each time the reference count goes from 1 to 0.  However,
unlike the main reference count, the acquire counter is only modified while
holding some additional locks.

Increments to the acquire counter are only done while holding either the
parent `TreeInode`'s `contents_` lock (in the case of
`TreeInode::getOrLoadChild()`) or the `InodeMap` lock (in the case of
`InodeMap::lookupInode()`).

Decrements to the acquire counter are only done while holding both the
parent `TreeInode`'s `contents_` lock and the `InodeMap` lock.

When thread A and thread B both see that the main reference count drops to 0,
they both attempt to acquire both the `TreeInode` and `InodeMap` locks.
Whichever thread acquires the locks first will see that the acquire count is
non-zero (since both threads incremented it when bumping the main reference
count from 0 to 1).  This thread decrements the acquire count and does nothing
else since the acquire count is non zero.  The second thread can then acquire
the locks, decrement the acquire count and see that it is now zero.  This
second thread can then perform the unload (while still holding both locks).

EdenMount Destruction
=====================

All inode objects store a pointer to the `EdenMount` that they are a part of.
This means that the `EdenMount` itself cannot be destroyed until all of its
inodes are destroyed.

We achieve this via the root `TreeInode`'s reference count.  During normal
operation, the `EdenMount` holds a reference to the root `TreeInode`
(technically the `InodeMap` holds the reference, but the `EdenMount` owns
the `InodeMap`). When the `EdenMount` needs to be destroyed, we release the
reference count on the root inode.  When the root inode becomes unreferenced we
know that all of its children have been destroyed, and it is now safe to
destroy the `EdenMount` object itself.

All of this is triggered through  the `EdenMount::destroy()` function.  This
function marks the mount as shutting down, which causes the `InodeMap` to
immediately unload any inodes that become newly unreferenced.  We then trigger
an immediate unload scan to unload any inodes that were already unreferenced.
Once this is done, we release the `InodeMap`'s reference count on the root
inode, allowing it to become unreferenced once all of its children are
destroyed.


FUSE Reference Counts
=====================

In addition to the reference count tracking how many `InodePtr` objects are
currently referring to an inode, `InodeBase` also keeps track of how many
outstanding references to this inode exist in the FUSE layer (this is the
number of `lookup()`/`create()`/`mkdir()`/`symlink()`/`link()` calls made for
this inode, minus the number of times it was forgotten via `forget()`).

However, the FUSE reference count is not directly related to the inode object
lifetime.

Inode objects may be unloaded even when the FUSE reference count is non-zero.
In this case, the `InodeMap` retains enough information needed to re-create the
`Inode` object if the inode number is later looked up again by the FUSE API.

The FUSE reference count is only adjusted while holding a normal InodePtr
reference to the inode.
