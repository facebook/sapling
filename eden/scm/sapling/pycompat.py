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


def identity(a):
    return a


import io

maplist = lambda *args: list(map(*args))
ziplist = lambda *args: list(zip(*args))
rawinput = input

sysargv = sys.argv

bytestr = str


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


def ensurestr(s):
    if isinstance(s, bytes):
        s = s.decode("utf-8")
    return s


def ensureunicode(s, errors="strict"):
    if not isinstance(s, str):
        s = s.decode("utf-8", errors=errors)
    return s


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


def getoptb(args, shortlist, namelist):
    return getopt.getopt(args, shortlist, namelist)


def gnugetoptb(args, shortlist, namelist):
    return getopt.gnu_getopt(args, shortlist, namelist)
