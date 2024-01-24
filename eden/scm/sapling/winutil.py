# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright (c) Mercurial Contributors.
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Utility functions related to Windows support, but which must be callable
on all platforms.
"""

from typing import Optional

from . import i18n


_ = i18n._
_winreservedchars = ':*?"<>|'
_winreservednames = {
    "con",
    "prn",
    "aux",
    "nul",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
}


def checkwinfilename(path: str) -> "Optional[str]":
    r"""Check that the base-relative path is a valid filename on Windows.
    Returns None if the path is ok, or a UI string describing the problem.

    >>> checkwinfilename("just/a/normal/path")
    >>> checkwinfilename("foo/bar/con.xml")
    "filename contains 'con', which is reserved on Windows"
    >>> checkwinfilename("foo/con.xml/bar")
    "filename contains 'con', which is reserved on Windows"
    >>> checkwinfilename("foo/bar/xml.con")
    >>> checkwinfilename("foo/bar/AUX/bla.txt")
    "filename contains 'AUX', which is reserved on Windows"
    >>> checkwinfilename("foo/bar/bla:.txt")
    "filename contains ':', which is reserved on Windows"
    >>> checkwinfilename("foo/bar/b\07la.txt")
    "filename contains '\\x07', which is invalid on Windows"
    >>> checkwinfilename("foo/bar/bla ")
    "filename ends with ' ', which is not allowed on Windows"
    >>> checkwinfilename("../bar")
    >>> checkwinfilename("foo\\")
    "filename ends with '\\', which is invalid on Windows"
    >>> checkwinfilename("foo\\/bar")
    "directory name ends with '\\', which is invalid on Windows"
    """
    if path.endswith("\\"):
        return _("filename ends with '\\', which is invalid on Windows")
    if "\\/" in path:
        return _("directory name ends with '\\', which is invalid on Windows")
    for n in path.replace("\\", "/").split("/"):
        if not n:
            continue
        for c in n:
            if c in _winreservedchars:
                return _("filename contains '%s', which is reserved " "on Windows") % c
            if ord(c) <= 31:
                return _("filename contains %r, which is invalid " "on Windows") % c
        base = n.split(".")[0]
        if base and base.lower() in _winreservednames:
            return _("filename contains '%s', which is reserved " "on Windows") % base
        t = n[-1:]
        if t in ". " and n not in "..":
            return _("filename ends with '%s', which is not allowed " "on Windows") % t
