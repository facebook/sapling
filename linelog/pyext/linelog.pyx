from libc.stdint cimport uint32_t, uint8_t

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
