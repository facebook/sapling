# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pycompat.py - portability shim for python 3
#
# Copyright Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

import errno
import getopt
import os
import shlex
import sys


ispypy = r"__pypy__" in sys.builtin_module_names

if sys.version_info[0] < 3:
    import cookielib

    import cPickle as pickle

    import httplib

    import Queue as _queue

    import SocketServer as socketserver
else:
    import http.cookiejar as cookielib  # noqa: F401
    import http.client as httplib  # noqa: F401
    import pickle  # noqa: F401
    import queue as _queue
    import socketserver  # noqa: F401

empty = _queue.Empty
queue = _queue


def identity(a):
    return a


if sys.version_info[0] >= 3:
    import builtins
    import functools
    import io
    import struct

    fsencode = os.fsencode
    fsdecode = os.fsdecode
    oslinesep = os.linesep
    osname = os.name
    ospathsep = os.pathsep
    ossep = os.sep
    osaltsep = os.altsep
    getcwd = os.getcwd
    sysplatform = sys.platform
    sysexecutable = sys.executable
    stringio = io.BytesIO
    maplist = lambda *args: list(map(*args))
    ziplist = lambda *args: list(zip(*args))
    rawinput = input
    range = range

    stdin = sys.stdin
    stdout = sys.stdout
    stderr = sys.stderr

    sysargv = sys.argv

    bytechr = chr
    bytestr = str
    iterbytestr = iter
    sysbytes = identity
    sysstr = identity
    strurl = identity
    bytesurl = identity

    def raisewithtb(exc, tb):
        """Raise exception with the given traceback"""
        raise exc.with_traceback(tb)

    def getdoc(obj):
        """Get docstring as bytes; may be None so gettext() won't confuse it
        with _('')"""
        if isinstance(obj, str):
            return obj
        doc = getattr(obj, u"__doc__", None)
        if doc is None:
            return doc
        return sysbytes(doc)

    unicode = str

    strkwargs = identity
    byteskwargs = identity
    shlexsplit = shlex.split

    def decodeutf8(s):
        # type: (bytes) -> str
        return s.decode("utf-8")


else:
    import cStringIO

    bytechr = chr
    bytestr = str
    iterbytestr = iter
    sysbytes = identity
    sysstr = identity
    strurl = identity
    bytesurl = identity
    range = xrange  # noqa: F821
    unicode = unicode

    # this can't be parsed on Python 3
    exec("def raisewithtb(exc, tb):\n" "    raise exc, None, tb\n")

    def fsencode(filename):
        """
        Partial backport from os.py in Python 3, which only accepts bytes.
        In Python 2, our paths should only ever be bytes, a unicode path
        indicates a bug.
        """
        if isinstance(filename, str):
            return filename
        else:
            raise TypeError("expect str, not %s" % type(filename).__name__)

    # In Python 2, fsdecode() has a very chance to receive bytes. So it's
    # better not to touch Python 2 part as it's already working fine.
    fsdecode = identity

    def getdoc(obj):
        if isinstance(obj, str):
            return obj
        return getattr(obj, "__doc__", None)

    def _getoptbwrapper(orig, args, shortlist, namelist):
        return orig(args, shortlist, namelist)

    strkwargs = identity
    byteskwargs = identity

    oslinesep = os.linesep
    osname = os.name
    ospathsep = os.pathsep
    ossep = os.sep
    osaltsep = os.altsep
    stdin = sys.stdin
    stdout = sys.stdout
    stderr = sys.stderr
    if getattr(sys, "argv", None) is not None:
        sysargv = sys.argv
    sysplatform = sys.platform
    getcwd = os.getcwd
    sysexecutable = sys.executable
    shlexsplit = shlex.split
    stringio = cStringIO.StringIO
    maplist = map
    ziplist = zip
    rawinput = raw_input  # noqa

    def decodeutf8(s):
        # type: (bytes) -> bytes
        assert isinstance(s, bytes)
        return s


isjython = sysplatform.startswith("java")

isdarwin = sysplatform == "darwin"
islinux = sysplatform.startswith("linux")
isposix = osname == "posix"
iswindows = osname == "nt"


def getoptb(args, shortlist, namelist):
    return _getoptbwrapper(getopt.getopt, args, shortlist, namelist)


def gnugetoptb(args, shortlist, namelist):
    return _getoptbwrapper(getopt.gnu_getopt, args, shortlist, namelist)


def getcwdsafe():
    """Returns the current working dir, or None if it has been deleted"""
    try:
        return getcwd()
    except OSError as err:
        if err.errno == errno.ENOENT:
            return None
        raise
