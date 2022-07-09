# EdenFS on Windows

On Windows, EdenFS uses Microsoft's [ProjectedFS][PrjFS] which works
significantly differently from [FUSE][FUSE] and [NFS][NFS] that it warrants its
own page. The rest of this document assumes prior knowledge about these two.

## Cached State

ProjectedFS was designed by Microsoft to have no overhead in
the common path: reading an already read or modified file. To achieve this, the
state of files is fully managed by ProjectedFS and is stored directly in the
working copy. EdenFS is only involved when providing the state of files that
ProjectedFS is not aware of.

For instance, the first time a file is being opened, ProjectedFS would first
send EdenFS a [`PRJ_GET_PLACEHOLDER_INFO_CB`][PRJ_GET_PLACEHOLDER_INFO_CB]
callback which will populate a placeholder file in the NTFS backing filesystem
by calling the [PrjWritePlaceholderInfo][PrjWritePlaceholderInfo] API.
Similarly, on the first read, the
[`PRJ_GET_FILE_DATA_CB`][PRJ_GET_FILE_DATA_CB] is sent to EdenFS. EdenFS would
then write the file content by calling [`PrjWriteFileData`][PrjWriteFileData]
which will write the file to the working copy, the file is now considered to be
a hydrated placeholder. Subsequent open or reads will not involve EdenFS as
these will be served from the filesystem directly.

While this allows for very fast reads to the working copy, it also leads to a
surprising behavior: **files that have been read once will still be readable
after EdenFS is stopped!**

One very important aspect of providing file data or metadata is that
ProjectedFS is the sole maintainer of the writeable working copy, and
thus EdenFS should only provide file data and metadata from the current
Mercurial commit. For instance, user created files should not be present in
directory enumeration, or more surprisingly, renamed files will always be
referred by ProjectedFS from their
[pre-rename path and name](https://github.com/microsoft/ProjFS-Managed-API/issues/68).
For this reason, EdenFS rely solely on Mercurial trees to serve
ProjectedFS callbacks and will not consult the [inode](Inodes.md)
state.

The rules are slightly different for directories as these will always be
queried even after the first directory listing. ProjectedFS will use three
callbacks for directory listing, starting with
[`PRJ_START_DIRECTORY_ENUMERATION_CB`][PRJ_START_DIRECTORY_ENUMERATION_CB] to
open the directory. Reading it is done via the
[`PRJ_GET_DIRECTORY_ENUMERATION_CB`][PRJ_GET_DIRECTORY_ENUMERATION_CB] callback
and finally closing a directory is done via
[`PRJ_END_DIRECTORY_ENUMERATION_CB`][PRJ_END_DIRECTORY_ENUMERATION_CB]. Note
that directories that have been created and thus aren't present in the current
Mercurial commit will not be receiving these callbacks.

## Inode State

While EdenFS on Windows makes little use of the inode state, it is
still fundamental to EdenFS inner working. To name a few, `getScmStatus`,
`checkoutRevision` or `globFiles` all rely on the inode state as they care
about the working copy state that ProjectedFS doesn't provide.

### Write notifications

Whenever a write operation is performed in the working copy (writing a file,
renaming it, creating a directory, etc), the callback
[`PRJ_NOTIFICATION_CB`][PRJ_NOTIFICATION_CB] is invoked in EdenFS. This
callback is usually invoked after the write operation has taken place and thus
EdenFS cannot refuse the operation.

The most subtle part about this callback is that ProjectedFS doesn't
provide any guarantee about the ordering of them. For instance, during a
concurrent directory hierarchy creation, a notification on a child directory
may be received prior to the notification of its parent directory! The same is
true for file and directory removal.

In order for the inode state to stay in sync with the working copy
state, EdenFS handles all of the notification serially in a single background
thread. The handling of these notifications is done in a
non-blocking manner in EdenFS. On receiving a notification, EdenFS will first
inspect the state of the file/directory on which the notification occurs and
will then update the inode state accordingly: for a missing file,
it will remove it from inode hierarchy, for a missing directory, the entire
directory hierarchy will be removed, etc.

This scheme means that during write heavy workloads, the inode
state will always be lagging behind the working copy. Since EdenFS only needs
the query the inode state while servicing Thrift requests, EdenFS
only needs to make sure that the inode state caught up with all the
changes to the working copy prior to servicing the Thrift requests. This is
done by simply enqueuing an empty notification and waiting for it to be
serviced.

Since some clients ([Buck][Buck], [Watchman][Watchman]) often don't mind if the
data returned is slightly out of date, all the Thrift queries accept a
`SyncBehavior` argument that allows the client to control how long to wait for
the inode to be synchronized with the working copy. Note that this only
guarantees that all the writes made prior to the Thrift request have been
synced up, writes that race with the Thrift query are not guaranteed to be
synced up.

## Invalidations

As mentioned above, ProjectedFS will only trigger callbacks in EdenFS the first
time a file is read or opened, thus if during a checkout operation, a file that
has been read changes, that file will need to be invalidated. This is done via
the the [PrjDeleteFile][PrjDeleteFile] API. For directories, and as described
above, callbacks are only sent to directories present in the current commit,
and never sent to user created directories, thus EdenFS needs to add a
placeholder to them if the directory either changes, or is present in the
destination commit during the checkout operation. This is done via the
[PrjMarkDirectoryAsPlaceholder][PrjMarkDirectoryAsPlaceholder] API. While
Microsoft's documentation doesn't document this API to be used for
invalidation, VFSForGit is using it to perform invalidation in the same way as
EdenFS.

## Pitfalls and caveats

### Invalidations

Invalidation has been the source of several bugs in EdenFS. Starting with
passing a GUID that doesn't match the GUID of the root folder in
`PrjMarkDirectoryAsPlaceholder`. This sometimes leads to Windows throwing a
"The provider that supports file system virtualization is temporarily
unavailable" error. To avoid this issue, EdenFS stores the GUID used when
creating a mount in the mount configuration, and will use the same GUID for the
whole lifetime of the working copy.

Still on `PrjMarkDirectoryAsPlaceholder`, calling this API on a non-populated
directory will lead to recursive callbacks which have at times deadlocked
EdenFS due to trying to recursively take already held locks.

The `PrjDeleteFile` and [`PrjUpdateFileIfNeeded`][PrjUpdateFileIfNeeded] can
only be used on an empty directory, or they will fail claiming that the
directory isn't empty. While this is expected for the former, this is
surprising for the latter. During callbacks, ProjectedFS passes the relative
path of the file as well as the
[`PRJ_PLACEHOLDER_VERSION_INFO`][PRJ_PLACEHOLDER_VERSION_INFO] stored in the
placeholder (which can be populated via `PrjWritePlaceholderInfo`), and EdenFS
walks the Mercurial trees to serve the callback. An optimization would be
shortcut this walk by storing the tree/file ID in the placeholder and using it
to obtain the same data as the walk. Unfortunately, due to
`PrjUpdateFileIfNeeded` not being able to update the placeholder of directories
containing untracked files, placeholders would become out of date after
checkout operations, rendering them unuseable.

### Renaming directories

Due to the way ProjectedFS tracks the state of the working copy, it
unfortunately doesn't support renaming directory placeholders. This has been a
source of complaints from users, and the best remedy has been to teach them to
use `hg mv` instead of a plain `mv`.

### EdenFS can't prevent writes

As write notifications are being sent after the write to the working copy has
occured, EdenFS can thus not deny them and needs to honor it. In particular,
this means that EdenFS cannot prevent writes to its magic .eden/config file.

### Writing to the working copy is allowed when EdenFS is stopped

As noted above, the working copy stays available when EdenFS is stopped, and
even more surprising, writing to fully materialized files is also allowed when
EdenFS is stopped. Some users have reported editing files long after EdenFS has
stopped. At startup, EdenFS will scan the fully materialized directories to
update its overlay to stay in sync with the filesystem state.


[PrjFS]: https://docs.microsoft.com/en-us/windows/win32/projfs/projected-file-system
[FUSE]: https://en.wikipedia.org/wiki/Filesystem_in_Userspace
[NFS]: https://datatracker.ietf.org/doc/html/rfc1813
[NTFS]: https://en.wikipedia.org/wiki/NTFS
[PRJ_GET_PLACEHOLDER_INFO_CB]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nc-projectedfslib-prj_get_placeholder_info_cb
[PRJ_GET_FILE_DATA_CB]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nc-projectedfslib-prj_get_file_data_cb
[PrjWriteFileData]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nf-projectedfslib-prjwritefiledata
[PrjWritePlaceholderInfo]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nf-projectedfslib-prjwriteplaceholderinfo
[PRJ_NOTIFICATION_CB]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nc-projectedfslib-prj_notification_cb
[Buck]: https://buck.build
[Watchman]: https://facebook.github.io/watchman/
[PrjDeleteFile]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nf-projectedfslib-prjdeletefile
[PrjUpdateFileIfNeeded]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nf-projectedfslib-prjupdatefileifneeded
[PRJ_START_DIRECTORY_ENUMERATION_CB]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nc-projectedfslib-prj_start_directory_enumeration_cb
[PRJ_GET_DIRECTORY_ENUMERATION_CB]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nc-projectedfslib-prj_get_directory_enumeration_cb
[PRJ_END_DIRECTORY_ENUMERATION_CB]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nc-projectedfslib-prj_end_directory_enumeration_cb
[PrjMarkDirectoryAsPlaceholder]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/nf-projectedfslib-prjmarkdirectoryasplaceholder
[PRJ_PLACEHOLDER_VERSION_INFO]: https://docs.microsoft.com/en-us/windows/win32/api/projectedfslib/ns-projectedfslib-prj_placeholder_version_info
