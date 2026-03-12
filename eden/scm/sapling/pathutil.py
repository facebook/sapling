# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2013 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno
import os
import posixpath
import stat

import bindings

from . import encoding, error, identity, util
from .i18n import _


def _lowerclean(s):
    return encoding.hfsignoreclean(s.lower())


class pathauditor:
    """ensure that a filesystem path contains no banned components.
    the following properties of a path are checked:

    - ends with a directory separator
    - contains a dotdir: ".hg", ".sl", ".git"
    - starts at the root of a windows drive
    - contains ".."

    More check are also done about the file system states:
    - traverses a symlink (e.g. a/symlink_here/b)
    """

    def __init__(self, root, cached=False):
        self._inner = bindings.checkout.pathauditor(root)

    def __call__(self, path, mode=None):
        """Check the relative path.
        path may contain a pattern (e.g. foodir/**.txt)"""
        try:
            self._inner.audit(path)
        except Exception as ex:
            # Re-raise with error.Abort type
            raise error.Abort(str(ex))

    def check(self, path):
        try:
            self(path)
            return True
        except (OSError, error.Abort):
            return False


def canonpath(root, cwd, myname):
    """return the canonical path of myname, given cwd and root

    >>> def check(root, cwd, myname):
    ...     try:
    ...         return canonpath(root, cwd, myname)
    ...     except error.Abort:
    ...         return 'aborted'
    >>> def unixonly(root, cwd, myname, expected='aborted'):
    ...     if util.iswindows:
    ...         return expected
    ...     return check(root, cwd, myname)
    >>> def winonly(root, cwd, myname, expected='aborted'):
    ...     if not util.iswindows:
    ...         return expected
    ...     return check(root, cwd, myname)
    >>> winonly('d:\\\\repo', 'c:\\\\dir', 'filename')
    'aborted'
    >>> winonly('c:\\\\repo', 'c:\\\\dir', 'filename')
    'aborted'
    >>> winonly('c:\\\\repo', 'c:\\\\', 'filename')
    'aborted'
    >>> winonly('c:\\\\repo', 'c:\\\\', 'repo\\\\filename',
    ...         'filename')
    'filename'
    >>> winonly('c:\\\\repo', 'c:\\\\repo', 'filename', 'filename')
    'filename'
    >>> winonly('c:\\\\repo', 'c:\\\\repo\\\\subdir', 'filename',
    ...         'subdir/filename')
    'subdir/filename'
    >>> unixonly('/repo', '/dir', 'filename')
    'aborted'
    >>> unixonly('/repo', '/', 'filename')
    'aborted'
    >>> unixonly('/repo', '/', 'repo/filename', 'filename')
    'filename'
    >>> unixonly('/repo', '/repo', 'filename', 'filename')
    'filename'
    >>> unixonly('/repo', '/repo/subdir', 'filename', 'subdir/filename')
    'subdir/filename'
    """
    if util.endswithsep(root):
        rootsep = root
    else:
        rootsep = root + os.sep
    name = myname
    if not os.path.isabs(name):
        name = os.path.join(root, cwd, name)
    name = os.path.normpath(name)
    if name != rootsep and name.startswith(rootsep):
        name = name[len(rootsep) :]
        return util.pconvert(name)
    elif name == root:
        return ""
    else:
        # Determine whether `name' is in the hierarchy at or beneath `root',
        # by iterating name=dirname(name) until that causes no change (can't
        # check name == '/', because that doesn't work on windows). The list
        # `tail' holds the reversed list of components making up the relative
        # file name we want.
        tail = []
        head = name
        while True:
            try:
                s = util.samefile(head, root)
            except OSError:
                s = False
            if s:
                if not tail:
                    # name was actually the same as root (maybe a symlink)
                    return ""
                tail.reverse()
                name = os.path.join(*tail)
                return util.pconvert(name)
            dirname, basename = util.split(head)
            if dirname == head:
                break
            tail.append(basename)
            head = dirname

        # At this point we know "name" doesn't appear to be under
        # "root". However, "name" could contain a symlink that points
        # into a subdirectory of "root". Try resolving the first
        # symlink and invoking ourself recursively to see if we end up
        # under "root". We don't want to resolve all symlinks at once
        # since later symlinks in "name" could be inside the repo, and
        # we don't want to resolve those.
        maybesymlink = head
        while tail:
            part = tail.pop()

            maybesymlink = os.path.join(maybesymlink, part)
            if os.path.islink(maybesymlink):
                # This realpath() call should probably use strict=True when available in Python 3.10.
                dest = os.path.realpath(maybesymlink)
                if dest == maybesymlink:
                    # If realpath couldn't resolve the symlink, bail.
                    break

                return canonpath(
                    root,
                    cwd,
                    os.path.join(
                        dest,
                        *reversed(tail),
                    ),
                )

        # A common mistake is to use -R, but specify a file relative to the repo
        # instead of cwd.  Detect that case, and provide a hint to the user.
        hint = None
        try:
            if cwd != root:
                canonpath(root, root, myname)
                relpath = util.pathto(root, cwd, "")
                if relpath[-1] == os.sep:
                    relpath = relpath[:-1]
                hint = _("consider using '--cwd %s'") % relpath
        except error.Abort:
            pass

        raise error.Abort(_("%s not under root '%s'") % (myname, root), hint=hint)


def normasprefix(path):
    """normalize the specified path as path prefix

    Returned value can be used safely for "p.startswith(prefix)",
    "p[len(prefix):]", and so on.

    For efficiency, this expects "path" argument to be already
    normalized by "os.path.normpath", "os.path.realpath", and so on.

    See also issue3033 for detail about need of this function.

    >>> normasprefix('/foo/bar').replace(os.sep, '/')
    '/foo/bar/'
    >>> normasprefix('/').replace(os.sep, '/')
    '/'
    """
    d, p = os.path.splitdrive(path)
    if len(p) != len(os.sep):
        return path + os.sep
    else:
        return path


# forward two methods from posixpath that do what we need, but we'd
# rather not let our internals know that we're thinking in posix terms
# - instead we'll let them be oblivious.
join = posixpath.join
dirname = posixpath.dirname
