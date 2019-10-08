# Copyright Facebook, Inc. 2019
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os

from . import util, vfs as vfsmod
from .i18n import _


_rangemask = 0x7FFFFFFF


class physicalfilesystem(object):
    def __init__(self, root, dirstate):
        self.root = root
        self.ui = dirstate._ui
        self.opener = vfsmod.vfs(
            root, expandpath=True, realpath=True, cacheaudited=False
        )

        # This is needed temporarily to enable an incremental migration of
        # functionality to this layer.
        self.dirstate = dirstate
        self.mtolog = self.ui.configint("experimental", "samplestatus")
        self.ltolog = self.mtolog

    def purge(self, match, keepfiles, removefiles, removedirs, removeignored, dryrun):
        """Deletes untracked files and directories from the filesystem.

          keepfiles: The list of files that should not be deleted. This is
            generally added files, or modified files from a second parent. It's
            useful for filesystems which don't have direct access to the working
            copy data.
          removefiles: Whether to delete untracked files.
          removedirs: Whether to delete empty directories.
          removeignored: Whether to delete ignored files and directories.
          dryrun: Whether to actually perform the delete.

        Returns a tuple of (files, dirs, errors) indicating files and
        directories that were deleted (or, if a dry-run, should be deleted) and
        any errors that were encountered.
        """
        errors = []
        join = self.dirstate._repo.wjoin

        def remove(remove_func, name):
            try:
                remove_func(join(name))
            except OSError:
                errors.append(_("%s cannot be removed") % name)

        files, dirs = findthingstopurge(
            self.dirstate, match, removefiles, removedirs, removeignored
        )

        files = list(files)
        if not dryrun:
            for f in files:
                remove(util.unlink, f)

        # Only evaluate dirs after deleting files, since the lazy evaluation
        # will be checking to see if the directory is empty.
        if not dryrun:
            resultdirs = []
            for f in dirs:
                resultdirs.append(f)
                remove(os.rmdir, f)
        else:
            resultdirs = list(dirs)

        return files, resultdirs, errors


def findthingstopurge(dirstate, match, findfiles, finddirs, includeignored):
    """Find files and/or directories that should be purged.

    Returns a pair (files, dirs), where files is an iterable of files to
    remove, and dirs is an iterable of directories to remove.
    """
    wvfs = dirstate._repo.wvfs
    if finddirs:
        directories = set(f for f in match.files() if wvfs.isdir(f))
        match.traversedir = directories.add

    status = dirstate.status(match, includeignored, False, True)

    if findfiles:
        files = sorted(status.unknown + status.ignored)
    else:
        files = []

    if finddirs:
        # Use a generator expression to lazily test for directory contents,
        # otherwise nested directories that are being removed would be counted
        # when in reality they'd be removed already by the time the parent
        # directory is to be removed.
        dirs = (
            f
            for f in sorted(directories, reverse=True)
            if (match(f) and not os.listdir(wvfs.join(f)))
        )
    else:
        dirs = []

    return files, dirs
