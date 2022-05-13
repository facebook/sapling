# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# cython: language_level=3str

"""patch rmdir

Check if a directory is empty before trying to call rmdir on it. This works
around some kernel issues.

Have no effect on Windows.
"""

IF UNAME_SYSNAME != "Windows":
    from edenscm.mercurial import (
        extensions,
        pycompat,
    )
    import os
    import errno

    cdef extern from "dirent.h":
        ctypedef struct DIR
        struct dirent:
            pass

        int openat(int dirfd, const char *path, int flags)
        DIR *fdopendir(int fd)
        DIR *opendir(const char *path)
        dirent *readdir(DIR *)
        int closedir(DIR *)

    cdef extern from "sys/fcntl.h":
        cdef enum:
            O_RDONLY "O_RDONLY"

    cdef int _countdirat(int dir_fd, const char *path):
        """return min(3, the number of entries inside a directory).
        return -1 if the directory cannot be opened.
        """
        cdef int fd = openat(dir_fd, path, O_RDONLY)
        cdef DIR *d = fdopendir(fd)
        return _countdir(d)

    cdef int _countdirpath(const char *path):
        """return min(3, the number of entries inside a directory).
        return -1 if the directory cannot be opened.
        """
        cdef DIR *d = opendir(path)
        return _countdir(d)

    cdef int _countdir(DIR *d):
        if d == NULL:
            return -1

        cdef dirent *e
        cdef int n = 0
        while True:
            e = readdir(d)
            if e == NULL:
                break
            else:
                n += 1
                if n > 2:
                    break
        closedir(d)
        # No need to close the fd, it is owned by the DIR object.
        return n

    def _rmdir(orig, path, dir_fd=None):
        path = str(path) # In case it is type Path
        path = pycompat.encodeutf8(path)
        if dir_fd is not None:
            n = _countdirat(dir_fd, path)
        else:
            n = _countdirpath(path)
        if n >= 3:
            # The number 3 is because most systems have "." and "..". For systems
            # without them, we fallback to the original rmdir, the behavior should
            # still be correct.
            # Choose a slightly different error message other than "Directory not
            # empty" so the test could notice the difference.
            raise OSError(errno.ENOTEMPTY, b'Non-empty directory: %r' % path)
        else:
            if dir_fd is None:
                # Python 2 doesn't have the dir_fd arg, so we can't just pass
                # dir_fd=None when it's None.
                return orig(path)
            else:
                return orig(path, dir_fd=dir_fd)

    def uisetup(ui):
        extensions.wrapfunction(os, 'rmdir', _rmdir)
