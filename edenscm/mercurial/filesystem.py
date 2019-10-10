# Copyright Facebook, Inc. 2019
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno
import os
import stat

from . import encoding, error, pathutil, util, vfs as vfsmod
from .i18n import _


_rangemask = 0x7FFFFFFF


class physicalfilesystem(object):
    def __init__(self, root, dirstate):
        self.root = root
        self.ui = dirstate._ui
        self.opener = vfsmod.vfs(
            root, expandpath=True, realpath=True, cacheaudited=False
        )

        # This is needed temporarily to enable an incremental migration of
        # functionality to this layer.
        self.dirstate = dirstate
        self.mtolog = self.ui.configint("experimental", "samplestatus")
        self.ltolog = self.mtolog

    def _ischanged(self, fn, st):
        try:
            t = self.dirstate._map[fn]
        except KeyError:
            t = ("?", 0, 0, 0)

        if st is None:
            return (fn, False, False)

        state = t[0]
        if state in "a?":
            return (fn, True, False)
        elif state in "mnr":
            # 'm' and 'n' states mean the dirstate is tracking the file, so
            # we need to check if it's modified.
            # 'r' means the dirstate thinks the file is removed, but because
            # we just encountered it in the walk we know it's not actually
            # deleted. pendingchanges() purpose is only to report if the
            # file is changed, so we check this file just like if it was
            # 'n', then the upper dirstate/workingcopy layer can decide to
            # report the file as 'r' if needed.

            # This is equivalent to 'state, mode, size, time = dmap[fn]' but not
            # written like that for performance reasons. dmap[fn] is not a
            # Python tuple in compiled builds. The CPython UNPACK_SEQUENCE
            # opcode has fast paths when the value to be unpacked is a tuple or
            # a list, but falls back to creating a full-fledged iterator in
            # general. That is much slower than simply accessing and storing the
            # tuple members one by one.
            mode = t[1]
            size = t[2]
            time = t[3]
            if size >= 0 and (
                (size != st.st_size and size != st.st_size & _rangemask)
                or ((mode ^ st.st_mode) & 0o100 and self.dirstate._checkexec)
            ):
                if self.mtolog > 0:
                    reasons = []
                    if size == -2:
                        reasons.append("exists in p2")
                    elif size != st.st_size:
                        reasons.append("size changed (%s -> %s)" % (size, st.st_size))
                        # See T39234759. Sometimes watchman returns 0 size
                        # (st.st_size) and we suspect it's incorrect.
                        # Do a double check with os.stat and log it.
                        if st.st_size == 0:
                            path = self.opener.join(fn)
                            try:
                                reasons.append(
                                    "os.stat size = %s" % os.stat(path).st_size
                                )
                            except Exception as ex:
                                reasons.append("os.stat failed (%s)" % ex)
                    if mode != st.st_mode:
                        reasons.append("mode changed (%s -> %s)" % (mode, st.st_mode))
                    self.ui.log("status", "M %s: %s" % (fn, ", ".join(reasons)))

                return (fn, True, False)
            elif time != st.st_mtime and time != st.st_mtime & _rangemask:
                if self.ltolog:
                    self.ltolog -= 1
                    reason = "mtime changed (%s -> %s)" % (time, st.st_mtime)
                    self.ui.log("status", "L %s: %s" % (fn, reason))
                return (fn, True, True)
            elif st.st_mtime == self.dirstate._lastnormaltime:
                # fn may have just been marked as normal and it may have
                # changed in the same second without changing its size.
                # This can happen if we quickly do multiple commits.
                # Force lookup, so we don't miss such a racy file change.
                if self.ltolog:
                    self.ltolog -= 1
                    reason = "mtime untrusted (%s)" % (st.st_mtime)
                    self.ui.log("status", "L %s: %s" % (fn, reason))
                return (fn, True, True)
            else:
                if self.dirstate._istreestate:
                    self.dirstate._map.clearneedcheck(fn)
                    self.dirstate._dirty = True
                return None
        else:
            raise error.ProgrammingError(
                "filesystem.walk should not yield state '%s' for '%s'" % (state, fn)
            )

    def pendingchanges(self, match=None, listignored=False):
        """Yields all the files that differ from the pristine tree.

        Returns an iterator of (string, bool, bool), where the string is the
        repo-rooted file path, the first bool is whether the file exists on disk
        or not, and the last bool is whether the file is in the lookup state and
        needs to be compared.

        The last bool will be dropped once lookup handling is moved into the
        filesystem layer.
        """
        dmap = self.dirstate._map
        dmap.preload()

        if match is None:
            match = util.always

        seen = set()
        for fn, st in self._walk(match, listignored):
            seen.add(fn)
            changed = self._ischanged(fn, st)
            if changed:
                yield changed

        auditpath = pathutil.pathauditor(self.root, cached=True)

        # Identify files that should exist but were not seen in the walk and
        # report them as changed.
        dget = dmap.__getitem__
        parentmf = None
        for fn in dmap:
            if fn in seen or not match(fn):
                continue
            t = dget(fn)
            size = t[2]

            # If it came from the other parent and it doesn't exist in p1,
            # ignore it here. We only want to report changes relative to the
            # pristine p1 tree. For hg status, the higher level dirstate will
            # add in anything that came from p2.
            if size == -2:
                if parentmf is None:
                    repo = self.dirstate._repo
                    p1 = self.dirstate.parents()[0]
                    pctx = repo[p1]
                    parentmf = pctx.manifest()
                if fn not in parentmf:
                    continue

            # We might not've seen a path because it's in a directory that's
            # ignored and the walk didn't go down that path. So let's double
            # check for the existence of that file.
            st = util.statfiles([self.opener.join(fn)])[0]

            # auditpath checks to see if the file is under a symlink directory.
            # If it is, we treat it the same as if it didn't exist.
            if st is None or not auditpath.check(fn):
                yield (fn, False, False)
            else:
                changed = self._ischanged(fn, st)
                if changed:
                    yield changed

    @util.timefunction("fswalk", 0, "ui")
    def _walk(self, match, listignored=False):
        join = self.opener.join
        listdir = util.listdir
        dirkind = stat.S_IFDIR
        regkind = stat.S_IFREG
        lnkkind = stat.S_IFLNK
        badfn = match.bad
        matchfn = match.matchfn
        matchalways = match.always()
        matchtdir = match.traversedir
        dmap = self.dirstate._map

        ignore = self.dirstate._ignore
        dirignore = self.dirstate._dirignore
        if listignored:
            ignore = util.never
            dirignore = util.never

        normalize = self.dirstate.normalize
        normalizefile = None
        if self.dirstate._checkcase:
            normalizefile = self.dirstate._normalizefile

        # Explicitly listed files circumvent the ignored matcher, so let's
        # record which directories we need to handle.
        # TODO: All ignore logic should be encapsulated in the matcher and
        # shouldn't be special cased here.
        explicitfiles = set(match.files())
        explicitdirs = set(util.dirs(explicitfiles))

        work = [""]
        wadd = work.append
        seen = set()
        while work:
            nd = work.pop()
            if not match.visitdir(nd) or nd == ".hg":
                continue
            skip = None
            if nd != "":
                skip = ".hg"
            try:
                entries = listdir(join(nd), stat=True, skip=skip)
            except OSError as inst:
                if inst.errno in (errno.EACCES, errno.ENOENT):
                    match.bad(nd, encoding.strtolocal(inst.strerror))
                    continue
                raise
            for f, kind, st in entries:
                if normalizefile:
                    # even though f might be a directory, we're only
                    # interested in comparing it to files currently in the
                    # dmap -- therefore normalizefile is enough
                    nf = normalizefile(nd and (nd + "/" + f) or f, True, True)
                else:
                    nf = nd and (nd + "/" + f) or f
                if nf not in seen:
                    seen.add(nf)
                    if kind == dirkind:
                        if not dirignore(nf) or nf in explicitdirs:
                            if matchtdir:
                                matchtdir(nf)
                            nf = normalize(nf, True, True)
                            wadd(nf)
                    elif matchalways or matchfn(nf):
                        if kind == regkind or kind == lnkkind:
                            if nf in dmap:
                                yield (nf, st)
                            elif not ignore(nf):
                                # unknown file
                                yield (nf, st)
                        else:
                            # This can happen for unusual file types, like named
                            # piped. We treat them as if they were missing, so
                            # report them as missing. Covered in test-symlinks.t
                            if nf in explicitfiles:
                                badfn(nf, badtype(kind))

    def purge(self, match, keepfiles, removefiles, removedirs, removeignored, dryrun):
        """Deletes untracked files and directories from the filesystem.

          keepfiles: The list of files that should not be deleted. This is
            generally added files, or modified files from a second parent. It's
            useful for filesystems which don't have direct access to the working
            copy data.
          removefiles: Whether to delete untracked files.
          removedirs: Whether to delete empty directories.
          removeignored: Whether to delete ignored files and directories.
          dryrun: Whether to actually perform the delete.

        Returns a tuple of (files, dirs, errors) indicating files and
        directories that were deleted (or, if a dry-run, should be deleted) and
        any errors that were encountered.
        """
        errors = []
        join = self.dirstate._repo.wjoin

        def remove(remove_func, name):
            try:
                remove_func(join(name))
            except OSError:
                errors.append(_("%s cannot be removed") % name)

        files, dirs = findthingstopurge(
            self.dirstate, match, removefiles, removedirs, removeignored
        )

        files = list(files)
        if not dryrun:
            for f in files:
                remove(util.unlink, f)

        # Only evaluate dirs after deleting files, since the lazy evaluation
        # will be checking to see if the directory is empty.
        if not dryrun:
            resultdirs = []
            for f in dirs:
                resultdirs.append(f)
                remove(os.rmdir, f)
        else:
            resultdirs = list(dirs)

        return files, resultdirs, errors


def findthingstopurge(dirstate, match, findfiles, finddirs, includeignored):
    """Find files and/or directories that should be purged.

    Returns a pair (files, dirs), where files is an iterable of files to
    remove, and dirs is an iterable of directories to remove.
    """
    wvfs = dirstate._repo.wvfs
    if finddirs:
        directories = set(f for f in match.files() if wvfs.isdir(f))
        match.traversedir = directories.add

    status = dirstate.status(match, includeignored, False, True)

    if findfiles:
        files = sorted(status.unknown + status.ignored)
    else:
        files = []

    if finddirs:
        # Use a generator expression to lazily test for directory contents,
        # otherwise nested directories that are being removed would be counted
        # when in reality they'd be removed already by the time the parent
        # directory is to be removed.
        dirs = (
            f
            for f in sorted(directories, reverse=True)
            if (match(f) and not os.listdir(wvfs.join(f)))
        )
    else:
        dirs = []

    return files, dirs


def badtype(mode):
    kind = _("unknown")
    if stat.S_ISCHR(mode):
        kind = _("character device")
    elif stat.S_ISBLK(mode):
        kind = _("block device")
    elif stat.S_ISFIFO(mode):
        kind = _("fifo")
    elif stat.S_ISSOCK(mode):
        kind = _("socket")
    elif stat.S_ISDIR(mode):
        kind = _("directory")
    return _("unsupported file type (type is %s)") % kind
