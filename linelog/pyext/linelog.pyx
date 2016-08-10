from libc.stdint cimport uint32_t, uint8_t
from libc.stdlib cimport free, realloc
from libc.string cimport memcpy, memset
from posix cimport unistd

cdef extern from "../linelog.c":
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

cdef size_t pagesize = <size_t>unistd.sysconf(unistd._SC_PAGESIZE)
cdef size_t unitsize = pagesize # used when resizing a buffer

class LinelogError(Exception):
    _messages = {
        LINELOG_RESULT_EILLDATA: 'Illegal data',
        LINELOG_RESULT_ENOMEM: 'Out of memory',
        LINELOG_RESULT_EOVERFLOW: 'Overflow',
    }

    def __init__(self, result):
        self.result = result

    def __str__(self):
        return self._messages.get(self.result, 'Unknown error %d' % self.result)

cdef class _buffer: # thin wrapper around linelog_buf
    cdef linelog_buf buf;

    def __cinit__(self):
        memset(&self.buf, 0, sizeof(linelog_buf))

    cdef resize(self, size_t newsize):
        raise NotImplementedError()

    cdef flush(self):
        pass

    cdef close(self):
        pass

    cdef copyfrom(self, _buffer rhs):
        if rhs.buf.size == 0:
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

    cdef _eval(self, linelogfunc):
        # linelogfunc should be a function returning linelog_result, which
        # will be handled smartly: for LINELOG_RESULT_ENEEDRESIZE, resize
        # automatically and retry. for other errors, raise them.
        while True:
            result = linelogfunc()
            if result == LINELOG_RESULT_OK:
                return
            elif result == LINELOG_RESULT_ENEEDRESIZE:
                self.resize((self.buf.neededsize / unitsize + 1) * unitsize)
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
