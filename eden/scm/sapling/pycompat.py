# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pycompat.py - portability shim for python 3
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

import abc
import errno
import getopt
import os
import shlex
import sys


ispypy = r"__pypy__" in sys.builtin_module_names

import http.client as httplib  # noqa: F401
import http.cookiejar as cookielib  # noqa: F401
import pickle  # noqa: F401
import queue as _queue
import socketserver  # noqa: F401

empty = _queue.Empty
# pyre-fixme[11]: Annotation `_queue` is not defined as a type.
queue = _queue

basestring = tuple({type(""), type(b""), type("")})


def identity(a):
    return a


# Copied from util.py to avoid pycompat depending on Mercurial modules
if "TESTTMP" in os.environ or "testutil" in sys.modules:

    def istest():
        return True

else:

    def istest():
        return False


import io

oslinesep = os.linesep
osname = os.name
ospathsep = os.pathsep
ossep = os.sep
osaltsep = os.altsep
getcwd = os.getcwd
sysplatform = sys.platform
sysexecutable = sys.executable

stringio = io.BytesIO
stringutf8io = io.StringIO
maplist = lambda *args: list(map(*args))
ziplist = lambda *args: list(zip(*args))
rawinput = input
range = range

stdin = sys.stdin.buffer
stdout = sys.stdout.buffer
stderr = sys.stderr.buffer

sysargv = sys.argv

bytechr = chr
bytestr = str
buffer = memoryview


def raisewithtb(exc, tb):
    """Raise exception with the given traceback"""
    raise exc.with_traceback(tb)


def getdoc(obj):
    """Get docstring as bytes; may be None so gettext() won't confuse it
    with _('')"""
    if isinstance(obj, str):
        return obj
    doc = getattr(obj, "__doc__", None)
    return doc


unicode = str
shlexsplit = shlex.split


def encodeutf8(s, errors="strict"):
    return s.encode("utf-8", errors=errors)


def decodeutf8(s: bytes, errors: str = "strict") -> str:
    return s.decode("utf-8", errors=errors)


def iteritems(s):
    return s.items()


def listitems(s):
    return list(s.items())


def iterkeys(s):
    return s.keys()


def itervalues(s):
    return s.values()


def ensurestr(s):
    if isinstance(s, bytes):
        s = s.decode("utf-8")
    return s


def ensureunicode(s, errors="strict"):
    if not isinstance(s, str):
        s = s.decode("utf-8", errors=errors)
    return s


def toutf8lossy(value: str) -> str:
    return value


def inttobyte(value):
    return bytes([value])


def isint(i):
    return isinstance(i, int)


def parse_email(fp):
    # Rarely used, so let's lazy load it
    import email.parser
    import io

    ep = email.parser.Parser()
    # disable the "universal newlines" mode, which isn't binary safe.
    # Note, although we specific ascii+surrogateescape decoding here, we don't have
    # to specify it elsewhere for reencoding as the email.parser detects the
    # surrogates and automatically chooses the appropriate encoding.
    # See: https://github.com/python/cpython/blob/3.8/Lib/email/message.py::get_payload()
    fp = io.TextIOWrapper(
        fp, encoding=r"ascii", errors=r"surrogateescape", newline=chr(10)
    )
    try:
        return ep.parse(fp)
    finally:
        fp.detach()


ABC = abc.ABC
import collections.abc

Mapping = collections.abc.Mapping
Set = collections.abc.Set


isjython = sysplatform.startswith("java")

isdarwin = sysplatform == "darwin"
islinux = sysplatform.startswith("linux")
isposix = osname == "posix"
iswindows = osname == "nt"


def getoptb(args, shortlist, namelist):
    return getopt.getopt(args, shortlist, namelist)


def gnugetoptb(args, shortlist, namelist):
    return getopt.gnu_getopt(args, shortlist, namelist)


def getcwdsafe():
    """Returns the current working dir, or None if it has been deleted"""
    try:
        return getcwd()
    except OSError as err:
        if err.errno == errno.ENOENT:
            return None
        raise
