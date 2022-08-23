# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# distutils: language = c++
# cython: language_level=3str

# traceprof.pyx - C++ to Python bridge for the traceprof Mercurial extension

"""accurate callgraph profiling

lsprof's high precision, plus statprof's intuitive output format.

Config::

    [traceprof]
    # whether to disable Python GC before profiling
    disablegc = no

    # minimal microseconds to show a function
    timethreshold = 2000

    # minimal call count to show "(N times)"
    countthreshold = 2

    # frame de-duplication (slower to print outputs)
    framededup = yes
"""

from libc.stdio cimport fopen, fclose, FILE
from cpython.object cimport PyObject

import contextlib
import gc
import os
import tempfile

from edenscm.mercurial import pycompat

cdef extern from "edenscm/ext/extlib/traceprofimpl.cpp":
    void enable()
    void disable()
    void report(FILE *)
    void settimethreshold(double)
    void setcountthreshold(size_t)
    void setdedup(int)
    void clear()

cdef extern from "Python.h":
    FILE* PyFile_AsFile(PyObject *p)

@contextlib.contextmanager
def profile(ui, fp, section="profiling"):
    if ui is not None:
        if ui.configbool('traceprof', 'disablegc'):
            gc.disable() # slightly more predictable
        microseconds = ui.configint('traceprof', 'timethreshold')
        if microseconds is not None:
            settimethreshold((<double>microseconds) / 1000.0)
        count = ui.configint('traceprof', 'countthreshold')
        if count is not None:
            setcountthreshold(count)
        dedup = ui.configbool('traceprof', 'framededup', True)
        setdedup(<int>dedup)
    enable()
    try:
        yield
    finally:
        disable()
        # "report" only accepts a real file. "fp" could be stringio.
        # Therefore always use a temporary file as a buffer.
        pyfd, filename = tempfile.mkstemp("traceprof")
        os.close(pyfd)
        # Somehow the file handlers between Cython and CPython can be
        # incompatible on Windows (linked with different CRTs?). Using
        # the file handlers like `fdopen`, `PyFile_AsFile` would segfault
        # on Windows. Workaround that by using `fopen` provided by Cython
        # so only the Cython version of the file handlers are used.
        cfp = fopen(pycompat.encodeutf8(filename), "w")
        report(cfp)
        fclose(cfp)
        content = open(filename).read()
        os.unlink(filename)
        fp.write(content)
        clear()
