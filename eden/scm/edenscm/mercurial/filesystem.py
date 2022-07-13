# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import stat
from typing import Callable, Iterable, Optional, Tuple

from bindings import workingcopy
from edenscm.mercurial import match as matchmod, progress

from . import encoding, error, pathutil, util, vfs as vfsmod
from .i18n import _
from .node import hex


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
        self.mtolog = self.ui.configint("experimental", "samplestatus", 0)
        self.ltolog = self.mtolog
        self.dtolog = self.mtolog
        self.ftolog = self.mtolog
        self.cleanlookups = []

    def _ischanged(self, fn, st, lookups):
        try:
            t = self.dirstate._map[fn]
        except KeyError:
            t = ("?", 0, 0, 0)

        if st is None:
            return (fn, False)

        state = t[0]
        if state in "a?":
            return (fn, True)
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

                return (fn, True)
            elif (
                time != st.st_mtime and time != st.st_mtime & _rangemask
            ) or st.st_mtime == self.dirstate._lastnormaltime:
                if self.ltolog:
                    self.ltolog -= 1
                    if st.st_mtime == self.dirstate._lastnormaltime:
                        reason = "mtime untrusted (%s)" % (st.st_mtime)
                    else:
                        reason = "mtime changed (%s -> %s)" % (time, st.st_mtime)
                    self.ui.log("status", "L %s: %s" % (fn, reason))

                lookups.append(fn)
                return None
            else:
                if self.dirstate._istreestate:
                    self.dirstate.clearneedcheck(fn)
                return False
        else:
            raise error.ProgrammingError(
                "filesystem.walk should not yield state '%s' for '%s'" % (state, fn)
            )

    def _compareondisk(self, path, wctx, pctx):
        """Compares the on-disk file content with the clean-checkout content.
        Return True if on-disk is different, False if it is the same, and None
        of the on-disk file is deleted or no longer accessible.
        """
        try:
            # This will return True for a file that got replaced by a
            # directory in the interim, but fixing that is pretty hard.
            if path not in pctx or wctx.flags(path) != pctx.flags(path):
                # Has changed
                return True

            pfctx = pctx[path]
            # Normally we would use pfctx.cmp(wctx[path]) instead of the filelog
            # cmp() function. Unfortunately filectx.cmp accesses the file size,
            # and getting the size with the current data store requires fetching
            # the whole file content. For now let's avoid that and go straight
            # to the filelog.cmp. Later we will fix this comparison to be pure
            # content sha comparisons, which will remove the need for this hack.
            return pfctx.filelog().cmp(pfctx.filenode(), wctx[path].data())
        except (IOError, OSError):
            # A file become inaccessible in between? Mark it as deleted,
            # matching dirstate behavior (issue5584).
            # The dirstate has more complex behavior around whether a
            # missing file matches a directory, etc, but we don't need to
            # bother with that: if f has made it to this point, we're sure
            # it's in the dirstate.
            return None

    def pendingchanges(
        self, match: "Optional[Callable[[str], bool]]" = None, listignored: bool = False
    ) -> "Iterable[Tuple[str, bool]]":
        """Yields all the files that differ from the pristine tree.

        Returns an iterator of (string, bool), where the string is the
        repo-rooted file path and the bool is whether the file exists on disk
        or not.
        """
        results = []
        for fn in self._pendingchanges(match, listignored):
            results.append(fn[0])
            yield fn

        oldid = self.dirstate.identity()
        self._postpendingfixup(oldid, results)

    def _pendingchanges(self, match, listignored):
        dmap = self.dirstate._map
        dmap.preload()

        if match is None:
            match = util.always

        seen = set()

        walkfn = self._walk
        if self.ui.configbool("workingcopy", "enablerustwalker"):
            walkfn = self._rustwalk

        lookups = []
        with progress.bar(self.ui, _("scanning files"), _("files")) as prog:
            count = 0
            for fn, st in walkfn(match, listignored):
                count += 1
                prog.value = (count, fn)
                seen.add(fn)
                changed = self._ischanged(fn, st, lookups)
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
            state = t[0]
            size = t[2]

            # If it came from the other parent and it doesn't exist in p1,
            # ignore it here. We only want to report changes relative to the
            # pristine p1 tree. For hg status, the higher level dirstate will
            # add in anything that came from p2.
            if size == -2:
                if parentmf is None:
                    repo = self.dirstate._repo
                    p1 = self.dirstate.p1()
                    pctx = repo[p1]
                    parentmf = pctx.manifest()
                if fn not in parentmf:
                    continue

            # We might not've seen a path because it's in a directory that's
            # ignored and the walk didn't go down that path. So let's double
            # check for the existence of that file.
            st = list(util.statfiles([self.opener.join(fn)]))[0]

            # auditpath checks to see if the file is under a symlink directory.
            # If it is, we treat it the same as if it didn't exist.
            if st is None or not auditpath.check(fn):
                # Don't report it as deleted if it wasn't in the original tree,
                # because pendingchanges is only supposed to report differences
                # from the original tree. The higher level dirstate code will
                # handle testing if added files are still there.
                if state in "a":
                    continue
                yield (fn, False)
            else:
                changed = self._ischanged(fn, st, lookups)
                if changed:
                    yield changed

        for changed in self._processlookups(lookups):
            yield changed

    @util.timefunction("fswalk", 0, "ui")
    def _rustwalk(self, match, listignored=False):
        join = self.opener.join
        if not listignored:
            # Have the matcher skip ignored files. Technically exact files
            # provided by the user should be returned even if they're ignored.
            # The differencematcher handles this and returns True for exact
            # matches, even if they should be subtracted.
            origmatch = matchmod.differencematcher(match, self.dirstate._ignore)
            normalize = self.dirstate.normalize

            class normalizematcher(object):
                def visitdir(self, path):
                    return origmatch.visitdir(normalize(path))

                def __call__(self, path):
                    return origmatch(normalize(path))

                def bad(self, path, msg):
                    return origmatch.bad(path, msg)

            match = normalizematcher()
            match.traversedir = origmatch.traversedir

        traversedir = bool(match.traversedir)
        threadcount = self.ui.configint("workingcopy", "rustwalkerthreads")
        walker = workingcopy.walker(join(""), match, traversedir, threadcount)
        for fn in walker:
            fn = self.dirstate.normalize(fn)
            st = util.lstat(join(fn))
            if traversedir and stat.S_ISDIR(st.st_mode):
                match.traversedir(fn)
            else:
                yield fn, st

        # Sorted for test stability
        for path, walkerror in sorted(walker.errors()):
            # Warn about non-utf8 errors, but don't report them as bad.
            # Ideally we'd inspect the error type, but it's lost coming from
            # Rust. When this moves to Rust it will get easier.
            if walkerror == "invalid file name encoding":
                self.ui.warn(_("skipping invalid utf-8 filename: '%s'\n") % path)
                continue
            match.bad(path, walkerror)

    def _processlookups(self, lookups):
        repo = self.dirstate._repo
        p1 = self.dirstate.p1()

        if util.safehasattr(repo, "fileservice"):
            p1mf = repo[p1].manifest()
            lookupmatcher = matchmod.exact(repo.root, repo.root, lookups)
            # We fetch history because we know _compareondisk() uses
            # filelog.cmp() which computes the sha(p1, p2, text), which requires
            # the history of the file. Later we'll move to comparing content
            # hashes, and we can prefetch those hashes instead.
            # Note, this may be slow for files with long histories.
            repo.fileservice.prefetch(
                list((f, hex(p1mf[f])) for f in p1mf.matches(lookupmatcher)),
                fetchdata=False,
                fetchhistory=True,
            )

        wctx = repo[None]
        pctx = repo[p1]

        with progress.bar(
            self.ui, _("checking changes"), _("files"), len(lookups)
        ) as prog:
            # Sort so we get deterministic ordering. This is important for tests.
            count = 0
            for fn in sorted(lookups):
                count += 1
                prog.value = (count, fn)
                changed = self._compareondisk(fn, wctx, pctx)
                if changed is None:
                    # File no longer exists
                    if self.dtolog > 0:
                        self.dtolog -= 1
                        self.ui.log("status", "R %s: checked in filesystem" % fn)
                    yield (fn, False)
                elif changed is True:
                    # File exists and is modified
                    if self.mtolog > 0:
                        self.mtolog -= 1
                        self.ui.log("status", "M %s: checked in filesystem" % fn)
                    yield (fn, True)
                else:
                    # File exists and is clean
                    if self.ftolog > 0:
                        self.ftolog -= 1
                        self.ui.log("status", "C %s: checked in filesystem" % fn)
                    self.cleanlookups.append(fn)

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
                if not util.isvalidutf8(f):
                    self.ui.warn(_("skipping invalid utf-8 filename: '%s'\n") % f)
                    continue

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

    def purge(self, match, removefiles, removedirs, removeignored, dryrun):
        """Deletes untracked files and directories from the filesystem.

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

    def _postpendingfixup(self, oldid, changed):
        """update dirstate for files that are actually clean"""
        if self.cleanlookups or self.dirstate._dirty:
            try:
                repo = self.dirstate._repo

                # Updating the dirstate is optional so we don't wait on the
                # lock.
                with repo.disableeventreporting(), repo.wlock(False):
                    # The dirstate may have been reloaded after the wlock
                    # was taken, so load it again.
                    newdirstate = repo.dirstate
                    if newdirstate.identity() == oldid:
                        self._marklookupsclean()

                        # write changes out explicitly, because nesting
                        # wlock at runtime may prevent 'wlock.release()'
                        # after this block from doing so for subsequent
                        # changing files
                        #
                        # This is a no-op if dirstate is not dirty.
                        tr = repo.currenttransaction()
                        newdirstate.write(tr)
                    else:
                        # in this case, writing changes out breaks
                        # consistency, because .hg/dirstate was
                        # already changed simultaneously after last
                        # caching (see also issue5584 for detail)
                        repo.ui.debug("skip marking lookups clean: identity mismatch\n")
            except error.LockError:
                pass

    def _marklookupsclean(self):
        dirstate = self.dirstate
        normal = dirstate.normal
        newdmap = dirstate._map
        cleanlookups = self.cleanlookups
        self.cleanlookups = []

        repo = dirstate._repo
        p1 = dirstate.p1()
        wctx = repo[None]
        pctx = repo[p1]

        with progress.bar(
            self.ui, _("marking clean"), _("files"), len(cleanlookups)
        ) as prog:
            count = 0
            for f in cleanlookups:
                count += 1
                prog.value = (count, f)
                # Only make something clean if it's already in a
                # normal state. Things in other states, like 'm'
                # merge state, should not be marked clean.
                entry = newdmap[f]
                if entry[0] == "n" and f not in newdmap.copymap and entry[2] != -2:
                    # It may have been a while since we added the
                    # file to cleanlookups, so double check that
                    # it's still clean.
                    if self._compareondisk(f, wctx, pctx) is False:
                        normal(f)


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


def badtype(mode: int) -> str:
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
