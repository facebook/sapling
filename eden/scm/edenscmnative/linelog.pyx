# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from libc.errno cimport errno
from libc.stdint cimport uint32_t, uint8_t
from libc.stdlib cimport malloc, free, realloc
from libc.string cimport memcpy, memset, strdup
IF UNAME_SYSNAME != "Windows":
    from posix cimport fcntl, mman, stat, unistd
    from posix.types cimport off_t

import os

cdef extern from "lib/linelog/linelog.c":
    # (duplicated) declarations for Cython, as Cython cannot parse .h file
    ctypedef uint32_t linelog_linenum
    ctypedef uint32_t linelog_revnum
    ctypedef uint32_t linelog_offset

    ctypedef int linelog_result
    cdef linelog_result LINELOG_RESULT_OK
    cdef linelog_result LINELOG_RESULT_ENOMEM
    cdef linelog_result LINELOG_RESULT_EILLDATA
    cdef linelog_result LINELOG_RESULT_EOVERFLOW
    cdef linelog_result LINELOG_RESULT_ENEEDRESIZE

    ctypedef struct linelog_buf:
        uint8_t *data
        size_t size
        size_t neededsize
    ctypedef struct linelog_lineinfo:
        linelog_revnum rev
        linelog_linenum linenum
        linelog_offset offset
    ctypedef struct linelog_annotateresult:
        linelog_lineinfo *lines
        linelog_linenum linecount
        linelog_linenum maxlinecount

    cdef void linelog_annotateresult_clear(linelog_annotateresult *ar)
    cdef linelog_result linelog_clear(linelog_buf *buf)
    cdef size_t linelog_getactualsize(const linelog_buf *buf)
    cdef linelog_revnum linelog_getmaxrev(const linelog_buf *buf)

    cdef linelog_result linelog_annotate(const linelog_buf *buf,
            linelog_annotateresult *ar, linelog_revnum rev)
    cdef linelog_result linelog_replacelines(linelog_buf *buf,
            linelog_annotateresult *ar, linelog_revnum brev,
            linelog_linenum a1, linelog_linenum a2,
            linelog_linenum b1, linelog_linenum b2)
    cdef linelog_result linelog_replacelines_vec(linelog_buf *buf,
            linelog_annotateresult *ar, linelog_revnum brev,
            linelog_linenum a1, linelog_linenum a2,
            linelog_linenum blinecount, const linelog_revnum *brevs,
            const linelog_linenum *blinenums)
    cdef linelog_result linelog_getalllines(linelog_buf *buf,
            linelog_annotateresult *ar, linelog_offset offset1,
            linelog_offset offset2)

IF UNAME_SYSNAME == "Windows":
    cdef size_t unitsize = 4096
ELSE:
    cdef size_t pagesize = <size_t>unistd.sysconf(unistd._SC_PAGESIZE)
    cdef size_t unitsize = pagesize # used when resizing a buffer

class LinelogError(Exception):
    _messages = {
        LINELOG_RESULT_EILLDATA: b'Illegal data',
        LINELOG_RESULT_ENOMEM: b'Out of memory',
        LINELOG_RESULT_EOVERFLOW: b'Overflow',
    }

    def __init__(self, result):
        self.result = result

    def __str__(self):
        return self._messages.get(self.result, b'Unknown error %d' % self.result)

cdef class _buffer: # thin wrapper around linelog_buf
    cdef linelog_buf buf

    def __cinit__(self):
        memset(&self.buf, 0, sizeof(linelog_buf))

    cdef resize(self, size_t newsize):
        raise NotImplementedError()

    cdef flush(self):
        pass

    cdef close(self):
        pass

    cdef copyfrom(self, _buffer rhs):
        if rhs.buf.size == 0 or &rhs.buf == &self.buf:
            return
        if rhs.buf.size > self.buf.size:
            self.resize(rhs.buf.size)
        memcpy(self.buf.data, <const void *>rhs.buf.data, rhs.buf.size)

    cdef getmaxrev(self):
        return linelog_getmaxrev(&self.buf)

    cdef getactualsize(self):
        return linelog_getactualsize(&self.buf)

    cdef clear(self):
        self._eval(lambda: linelog_clear(&self.buf))

    cdef annotate(self, linelog_annotateresult *ar, linelog_revnum rev):
        self._eval(lambda: linelog_annotate(&self.buf, ar, rev))

    cdef replacelines(self, linelog_annotateresult *ar, linelog_revnum brev,
                      linelog_linenum a1, linelog_linenum a2,
                      linelog_linenum b1, linelog_linenum b2):
        self._eval(lambda: linelog_replacelines(&self.buf, ar, brev, a1, a2,
                                                b1, b2))

    cdef replacelines_vec(self, linelog_annotateresult *ar,
                          linelog_revnum brev, linelog_linenum a1,
                          linelog_linenum a2, linelog_linenum blinecount,
                          const linelog_revnum *brevs,
                          const linelog_linenum *blinenums):
        self._eval(lambda: linelog_replacelines_vec(&self.buf, ar, brev,
                                                    a1, a2, blinecount,
                                                    brevs, blinenums))

    cdef getalllines(self, linelog_annotateresult *ar, linelog_offset offset1,
                     linelog_offset offset2):
        self._eval(lambda: linelog_getalllines(&self.buf, ar,
                                               offset1, offset2))

    cdef _eval(self, linelogfunc):
        # linelogfunc should be a function returning linelog_result, which
        # will be handled smartly: for LINELOG_RESULT_ENEEDRESIZE, resize
        # automatically and retry. for other errors, raise them.
        while True:
            result = linelogfunc()
            if result == LINELOG_RESULT_OK:
                return
            elif result == LINELOG_RESULT_ENEEDRESIZE:
                self.resize((self.buf.neededsize // unitsize + 1) * unitsize)
            else:
                raise LinelogError(result)

cdef class _memorybuffer(_buffer): # linelog_buf backed by memory
    def __dealloc__(self):
        self.close()

    cdef close(self):
        free(self.buf.data)
        memset(&self.buf, 0, sizeof(linelog_buf))

    cdef resize(self, size_t newsize):
        p = realloc(self.buf.data, newsize)
        if p == NULL:
            raise LinelogError(LINELOG_RESULT_ENOMEM)
        self.buf.data = <uint8_t *>p
        self.buf.size = newsize

cdef _excwitherrno(exctype, hint=None, filename=None):
    # like PyErr_SetFromErrno-family APIs but we avoid CPython APIs here
    # (so one day if Cython writes pypy/cffi code, this can be used as-is)
    message = os.strerror(errno)
    if hint is not None:
        message += b' (%s)' % hint
    return exctype(errno, message, filename)

IF UNAME_SYSNAME != "Windows":
    cdef class _filebuffer(_buffer): # linelog_buf backed by filesystem
        cdef int fd
        cdef size_t maplen
        cdef char *path

        def __cinit__(self, path):
            self.fd = -1
            self.maplen = 0
            self.path = strdup(path)
            if self.path == NULL:
                raise MemoryError()
            self._open()

        def __dealloc__(self):
            free(self.path)
            self.close()

        cdef resize(self, size_t newsize):
            if self.fd == -1:
                self._open()
            self._unmap()
            r = unistd.ftruncate(self.fd, <off_t>newsize)
            if r != 0:
                raise _excwitherrno(IOError, b'ftruncate')
            self._map()

        cdef flush(self):
            if self.buf.data == NULL:
                return
            r = mman.msync(self.buf.data, self.buf.size, mman.MS_ASYNC)
            if r != 0:
                raise _excwitherrno(OSError, b'msync')

        cdef close(self):
            self.flush()
            self._unmap()
            if self.fd == -1:
                return
            unistd.close(self.fd)
            self.fd = -1

        cdef _open(self):
            self.close()
            fd = fcntl.open(self.path, fcntl.O_RDWR | fcntl.O_CREAT, 0o644)
            if fd == -1:
                raise _excwitherrno(IOError, None, self.path)
            self.fd = fd
            self._map()

        cdef _map(self):
            assert self.fd != -1
            self._unmap()

            cdef stat.struct_stat st
            r = stat.fstat(self.fd, &st)
            if r != 0:
                raise _excwitherrno(IOError, b'fstat')

            cdef size_t filelen = <size_t>st.st_size
            self.maplen = (1 if filelen == 0 else filelen) # cannot be 0
            p = mman.mmap(NULL, self.maplen, mman.PROT_READ | mman.PROT_WRITE,
                          mman.MAP_SHARED, self.fd, 0)
            if p == NULL:
                raise _excwitherrno(OSError, b'mmap')

            self.buf.data = <uint8_t *>p
            self.buf.size = filelen

        cdef _unmap(self):
            if self.buf.data == NULL:
                return
            r = mman.munmap(self.buf.data, self.maplen)
            if r != 0:
                raise _excwitherrno(OSError, b'munmap')
            memset(&self.buf, 0, sizeof(linelog_buf))
            self.maplen = 0

cdef _ar2list(const linelog_annotateresult *ar):
    result = []
    cdef linelog_linenum i
    if ar != NULL:
        for i in range(0, ar.linecount):
            result.append((ar.lines[i].rev, ar.lines[i].linenum))
    return result

cdef class linelog:
    """Python wrapper around linelog"""

    cdef linelog_annotateresult ar
    cdef readonly _buffer buf
    cdef readonly bint closed
    cdef readonly object path

    def __cinit__(self):
        self.closed = 0
        memset(&self.ar, 0, sizeof(linelog_annotateresult))

    def __init__(self, path=None):
        """L(path : str?). Open a linelog.

        If path is empty or None, the linelog will be in-memory. Otherwise
        it's based on an on-disk file.

        The linelog object does not protect concurrent accesses to a same
        file. The caller should have some lock mechanism (like flock) to
        ensure one file is only accessed by one linelog object.
        """
        self.path = path
        if path:
            IF UNAME_SYSNAME == b'Windows':
                raise RuntimeError(b'on-disk linelog is unavailable on Windows')
            ELSE:
                self.buf = _filebuffer(path)
        else:
            self.buf = _memorybuffer()
        if self.buf.getactualsize() == 0:
            # initialize empty linelog automatically
            self.clear()
            self.annotate(0)

    def __dealloc__(self):
        self._clearannotateresult()

    def clear(self):
        """L.close() -> None. Close the file and free resources."""
        self._checkclosed()
        self._clearannotateresult()
        self.buf.clear()

    def flush(self):
        """L.flush() -> None. Flush changes to disk."""
        self._checkclosed()
        self.buf.flush()

    def close(self):
        """L.close() -> None. Close the file."""
        if self.closed:
            return
        self.buf.resize(self.buf.getactualsize())
        self.buf.close()
        self.closed = 1

    def copyfrom(self, rhs):
        """L.copyfrom(R : linelog) -> None

        Copy content from another linelog object."""
        assert isinstance(rhs, linelog)
        self._checkclosed()
        self.buf.copyfrom(rhs.buf)

    @property
    def maxrev(self):
        """L.maxrev() -> int. Return the max revision number."""
        self._checkclosed()
        return self.buf.getmaxrev()

    @property
    def actualsize(self):
        """L.maxrev() -> int. Return bytes used by the linelog."""
        self._checkclosed()
        return self.buf.getactualsize()

    def annotate(self, rev):
        """L.annotate(rev : int) -> None

        Annotate lines for specified revision. The result can be obtained
        via L.annotateresult.
        """
        self._checkclosed()
        try:
            self.buf.annotate(&self.ar, rev)
        except LinelogError:
            self._clearannotateresult()
            raise

    def replacelines(self, rev, a1, a2, b1, b2):
        """L.replacelines(rev, a1, a2, b1, b2 : int) -> None

        Replace lines[a1:a2] with lines[b1:b2] in rev. See comments above
        linelog_replacelines in linelog.h for details.
        """
        self._checkclosed()
        try:
            self.buf.replacelines(&self.ar, rev, a1, a2, b1, b2)
        except LinelogError:
            self._clearannotateresult()
            raise

    def replacelines_vec(self, rev, a1, a2, blines):
        """L.replacelines(rev, a1, a2 : int, blines: [(rev, linenum)]) -> None

        Replace lines[a1:a2] with blines. The change is marked as introduced
        by rev. See comments above linelog_replacelines_vec in linelog.h for
        details.
        """
        self._checkclosed()
        # prepare blinecount, brevs, blinenums
        cdef linelog_linenum i = 0, blinecount = <linelog_linenum>len(blines)
        cdef linelog_revnum *brevs = <linelog_revnum *>malloc(
            sizeof(linelog_revnum) * blinecount)
        cdef linelog_linenum *blinenums = <linelog_linenum *>malloc(
            sizeof(linelog_linenum) * blinecount)
        if blinecount > 0:
            assert brevs != NULL and blinenums != NULL
        for i in range(0, blinecount):
            brevs[i] = blines[i][0]
            blinenums[i] = blines[i][1]
        try:
            self.buf.replacelines_vec(&self.ar, rev, a1, a2,
                                      blinecount, brevs, blinenums)
        except LinelogError:
            self._clearannotateresult()
            raise
        finally:
            free(brevs)
            free(blinenums)

    @property
    def annotateresult(self):
        """L.annotateresult -> [(rev, linenum)]"""
        return _ar2list(&self.ar)

    def getalllines(self, offset1=0, offset2=0):
        """L.getalllines(offset1, offset2 : int) -> [(rev, linenum)]"""
        cdef linelog_annotateresult lines
        memset(&lines, 0, sizeof(linelog_annotateresult))
        self.buf.getalllines(&lines, offset1, offset2)
        result = _ar2list(&lines)
        linelog_annotateresult_clear(&lines)
        return result

    def getoffset(self, linenum):
        """L.getoffset(int) -> int"""
        if linenum > self.ar.linecount:
            raise IndexError(b'line number out of range')
        return self.ar.lines[linenum].offset

    cdef _checkclosed(self):
        if self.closed:
            raise ValueError(b'I/O operation on closed linelog')

    cdef _clearannotateresult(self):
        linelog_annotateresult_clear(&self.ar)

    def __repr__(self):
        return b'<%s linelog %s at 0x%x>' % (
            b'closed' if self.closed else b'open',
            b'(in-memory)' if self.path is None else repr(self.path),
            id(self)
        )
