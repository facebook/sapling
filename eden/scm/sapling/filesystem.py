# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import stat

from bindings import workingcopy

from . import util, vfs as vfsmod
from .i18n import _


class physicalfilesystem:
    def __init__(self, root, dirstate):
        self.root = root
        self.ui = dirstate._ui
        self.opener = vfsmod.vfs(
            root, expandpath=True, realpath=True, cacheaudited=False
        )

        # This is needed temporarily to enable an incremental migration of
        # functionality to this layer.
        self.dirstate = dirstate
        self.mtolog = self.ui.configint("experimental", "samplestatus", 0)
        self.ltolog = self.mtolog
        self.dtolog = self.mtolog
        self.ftolog = self.mtolog
        self.cleanlookups = []

    def purge(self, match, removefiles, removedirs, removeignored, dryrun):
        """Deletes untracked files and directories from the filesystem.

          removefiles: Whether to delete untracked files.
          removedirs: Whether to delete empty directories.
          removeignored: Whether to delete ignored files and directories.
          dryrun: Whether to actually perform the delete.

        Returns a tuple of (files, dirs, errors) indicating files and
        directories that were deleted (or, if a dry-run, should be deleted) and
        any errors that were encountered.
        """

        files, dirs, errors = findthingstopurge(
            self.dirstate, match, removefiles, removedirs, removeignored
        )

        join = self.dirstate._repo.wjoin

        def remove(remove_func, name):
            try:
                remove_func(join(name))
            except OSError:
                errors.append(_("%s cannot be removed") % name)

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

    status = dirstate.status(match, includeignored, False, True)

    if findfiles:
        files = sorted(status.unknown + status.ignored)
    else:
        files = []

    if finddirs:
        dirs, errors = _emptydirs(dirstate._ui, wvfs, dirstate, match)
    else:
        dirs, errors = [], []

    return files, dirs, errors


def _emptydirs(ui, wvfs, dirstate, match):
    directories = set(f for f in match.files() if wvfs.isdir(f))

    walker = workingcopy.walker(
        wvfs.base,
        ui.identity.dotdir(),
        match,
        True,
    )
    for fn in walker:
        fn = dirstate.normalize(fn)
        st = util.lstat(wvfs.join(fn))
        if stat.S_ISDIR(st.st_mode):
            directories.add(fn)

    # Use a generator expression to lazily test for directory contents,
    # otherwise nested directories that are being removed would be counted
    # when in reality they'd be removed already by the time the parent
    # directory is to be removed.
    dirs = (
        f
        for f in sorted(directories, reverse=True)
        if (match(f) and not os.listdir(wvfs.join(f)))
    )

    errors = ["%s: %s" % (_(msg), path) for path, msg in sorted(walker.errors())]

    return dirs, errors


def badtype(mode: int) -> str:
    kind = _("unknown")
    if stat.S_ISCHR(mode):
        kind = _("character device")
    elif stat.S_ISBLK(mode):
        kind = _("block device")
    elif stat.S_ISFIFO(mode):
        kind = _("fifo")
    elif stat.S_ISSOCK(mode):
        kind = _("socket")
    elif stat.S_ISDIR(mode):
        kind = _("directory")
    return _("unsupported file type (type is %s)") % kind
