# litemmap.pyx - read-only mmap implementation that does not keep a fd open
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import mmap as pymmap

IF UNAME_SYSNAME == "Windows":
    # Not supporting windows for now - fallback to Python's mmap
    mmap = pymmap.mmap
ELSE:
    from cpython.bytes cimport PyBytes_FromStringAndSize
    from libc.errno cimport errno
    from posix cimport mman, stat

    import os

    cdef extern from "sys/mman.h":
        cdef char *MAP_FAILED

    cdef _raise_oserror(message=''):
        if message:
            message += b': '
        message += os.strerror(errno)
        return OSError(errno, message)

    cdef class mmap:
        cdef char *ptr
        cdef size_t len

        def __cinit__(self, int fd, size_t length, int
                      access=pymmap.ACCESS_READ):
            cdef stat.struct_stat st
            # Only support read-only case
            if access != pymmap.ACCESS_READ:
                raise RuntimeError(b'access %s is unsupported' % access)

            # If length is 0, read size from file
            if length == 0:
                r = stat.fstat(fd, &st)
                if r != 0:
                    _raise_oserror(b'fstat failed')
                length = st.st_size

            self.ptr = <char*>mman.mmap(NULL, length, mman.PROT_READ,
                                        mman.MAP_SHARED, fd, 0)
            if self.ptr == MAP_FAILED:
                _raise_oserror(b'mmap failed')

            self.len = length

        cpdef close(self):
            if self.ptr == NULL:
                return

            r = mman.munmap(self.ptr, self.len)
            if r != 0:
                _raise_oserror(b'munmap failed')
            self.ptr = NULL

        def __getslice__(self, Py_ssize_t i, Py_ssize_t j):
            if self.ptr == NULL:
                raise RuntimeError(b'mmap closed')
            if i < 0:
                i = 0
            if j > self.len:
                j = self.len
            if i > j:
                i = j
            return PyBytes_FromStringAndSize(self.ptr + i, j - i)

        def __len__(self):
            return self.len

        def __dealloc__(self):
            self.close()

