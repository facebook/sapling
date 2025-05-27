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

from . import encoding, error, identity, util
from .i18n import _


def _lowerclean(s):
    return encoding.hfsignoreclean(s.lower())


class pathauditor:
    """ensure that a filesystem path contains no banned components.
    the following properties of a path are checked:

    - ends with a directory separator
    - under top-level .hg
    - starts at the root of a windows drive
    - contains ".."

    More check are also done about the file system states:
    - traverses a symlink (e.g. a/symlink_here/b)
    - inside a nested repository (a callback can be used to approve
      some nested repositories, e.g., subrepositories)

    The file system checks are only done when 'realfs' is set to True (the
    default). They should be disable then we are auditing path for operation on
    stored history.

    If 'cached' is set to True, audited paths and sub-directories are cached.
    Be careful to not keep the cache of unmanaged directories for long because
    audited paths may be replaced with symlinks.
    """

    def __init__(self, root, callback=None, realfs=True, cached=False):
        self.audited = set()
        self.auditeddir = set()
        self.root = root

        # Fall back to global identity for doc tests.
        ident = identity.sniffdir(root) or identity.default()
        self.dotdir = ident.dotdir()
        self.dotdirdot = self.dotdir + "."

        self._realfs = realfs
        self._cached = cached
        self.callback = callback
        if os.path.lexists(root) and not util.fscasesensitive(root):
            self.normcase = util.normcase
        else:
            self.normcase = lambda x: x

    def __call__(self, path, mode=None):
        """Check the relative path.
        path may contain a pattern (e.g. foodir/**.txt)"""

        path = util.localpath(path)
        normpath = self.normcase(path)
        if normpath in self.audited:
            return
        # AIX ignores "/" at end of path, others raise EISDIR.
        if util.endswithsep(path):
            raise error.Abort(_("path ends in directory separator: %s") % path)
        parts = util.splitpath(path)
        if (
            os.path.splitdrive(path)[0]
            or _lowerclean(parts[0]) in (self.dotdir, self.dotdirdot, "")
            or os.pardir in parts
        ):
            raise error.Abort(_("path contains illegal component: %s") % path)
        # Windows shortname aliases
        for p in parts:
            if "~" in p:
                first, last = p.split("~", 1)
                if last.isdigit() and first.upper() in ["HG", "HG8B6C", "SL", "SL8B6C"]:
                    raise error.Abort(_("path contains illegal component: %s") % path)
        if self.dotdir in _lowerclean(path):
            lparts = [_lowerclean(p.lower()) for p in parts]
            for p in self.dotdir, self.dotdirdot:
                if p in lparts[1:]:
                    pos = lparts.index(p)
                    base = os.path.join(*parts[:pos])
                    raise error.Abort(
                        _("path '%s' is inside nested repo %r") % (path, base)
                    )

        normparts = util.splitpath(normpath)
        assert len(parts) == len(normparts)

        parts.pop()
        normparts.pop()
        prefixes = []
        # It's important that we check the path parts starting from the root.
        # This means we won't accidentally traverse a symlink into some other
        # filesystem (which is potentially expensive to access).
        for i in range(len(parts)):
            prefix = os.sep.join(parts[: i + 1])
            normprefix = os.sep.join(normparts[: i + 1])
            if normprefix in self.auditeddir:
                continue
            if self._realfs:
                self._checkfs(prefix, path)
            prefixes.append(normprefix)

        if self._cached:
            self.audited.add(normpath)
            # only add prefixes to the cache after checking everything: we don't
            # want to add "foo/bar/baz" before checking if there's a "foo/.hg"
            self.auditeddir.update(prefixes)

    def _checkfs(self, prefix, path):
        """raise exception if a file system backed check fails"""
        curpath = os.path.join(self.root, prefix)
        try:
            st = os.lstat(curpath)
        except OSError as err:
            # EINVAL can be raised as invalid path syntax under win32.
            # They must be ignored for patterns can be checked too.
            if err.errno not in (errno.ENOENT, errno.ENOTDIR, errno.EINVAL):
                raise
        else:
            if stat.S_ISLNK(st.st_mode):
                msg = _("path %r traverses symbolic link %r") % (path, prefix)
                raise error.Abort(msg)
            elif stat.S_ISDIR(st.st_mode) and os.path.isdir(
                os.path.join(curpath, self.dotdir)
            ):
                if not self.callback or not self.callback(curpath):
                    msg = _("path '%s' is inside nested repo %r")
                    raise error.Abort(msg % (path, prefix))

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
