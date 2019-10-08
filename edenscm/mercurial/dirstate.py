# dirstate.py - working directory tracking for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import contextlib
import errno
import os
import stat
import weakref

from . import (
    encoding,
    error,
    filesystem,
    hintutil,
    match as matchmod,
    pathutil,
    perftrace,
    policy,
    pycompat,
    scmutil,
    treedirstate,
    treestate,
    txnutil,
    util,
)
from .i18n import _
from .node import hex, nullid


parsers = policy.importmod(r"parsers")

propertycache = util.propertycache
filecache = scmutil.filecache
_rangemask = 0x7FFFFFFF

dirstatetuple = parsers.dirstatetuple

slowstatuswarning = _(
    "(status will still be slow next time; try to complete or abort "
    "other source control operations and then run 'hg status' again)\n"
)


class repocache(filecache):
    """filecache for files in .hg/"""

    def join(self, obj, fname):
        return obj._opener.join(fname)


class rootcache(filecache):
    """filecache for files in the repository root"""

    def join(self, obj, fname):
        return obj._join(fname)


def _getfsnow(vfs):
    """Get "now" timestamp on filesystem"""
    tmpfd, tmpname = vfs.mkstemp()
    try:
        return os.fstat(tmpfd).st_mtime
    finally:
        os.close(tmpfd)
        vfs.unlink(tmpname)


class dirstate(object):
    def __init__(
        self,
        opener,
        ui,
        root,
        validate,
        repo,
        sparsematchfn=None,
        istreestate=False,
        istreedirstate=False,
    ):
        """Create a new dirstate object.

        opener is an open()-like callable that can be used to open the
        dirstate file; root is the root of the directory tracked by
        the dirstate.
        """
        self._opener = opener
        self._validate = validate
        self._root = root
        self._repo = weakref.proxy(repo)
        # ntpath.join(root, '') of Python 2.7.9 does not add sep if root is
        # UNC path pointing to root share (issue4557)
        self._rootdir = pathutil.normasprefix(root)
        self._dirty = False
        self._lastnormaltime = 0
        self._ui = ui
        self._filecache = {}
        self._parentwriters = 0
        self._filename = "dirstate"
        self._pendingfilename = "%s.pending" % self._filename
        self._plchangecallbacks = {}
        self._origpl = None
        self._updatedfiles = set()
        # TODO(quark): after migrating to treestate, remove legacy code.
        self._istreestate = istreestate
        self._istreedirstate = istreedirstate
        if istreestate:
            opener.makedirs("treestate")
            self._mapcls = treestate.treestatemap
        elif istreedirstate:
            self._mapcls = treedirstate.treedirstatemap
        else:
            self._mapcls = dirstatemap
        self._fs = filesystem.physicalfilesystem(root, self)

    @contextlib.contextmanager
    def parentchange(self):
        """Context manager for handling dirstate parents.

        If an exception occurs in the scope of the context manager,
        the incoherent dirstate won't be written when wlock is
        released.
        """
        self._parentwriters += 1
        yield
        # Typically we want the "undo" step of a context manager in a
        # finally block so it happens even when an exception
        # occurs. In this case, however, we only want to decrement
        # parentwriters if the code in the with statement exits
        # normally, so we don't have a try/finally here on purpose.
        self._parentwriters -= 1

    def beginparentchange(self):
        """Marks the beginning of a set of changes that involve changing
        the dirstate parents. If there is an exception during this time,
        the dirstate will not be written when the wlock is released. This
        prevents writing an incoherent dirstate where the parent doesn't
        match the contents.
        """
        self._ui.deprecwarn(
            "beginparentchange is obsoleted by the " "parentchange context manager.",
            "4.3",
        )
        self._parentwriters += 1

    def endparentchange(self):
        """Marks the end of a set of changes that involve changing the
        dirstate parents. Once all parent changes have been marked done,
        the wlock will be free to write the dirstate on release.
        """
        self._ui.deprecwarn(
            "endparentchange is obsoleted by the " "parentchange context manager.",
            "4.3",
        )
        if self._parentwriters > 0:
            self._parentwriters -= 1

    def pendingparentchange(self):
        """Returns true if the dirstate is in the middle of a set of changes
        that modify the dirstate parent.
        """
        return self._parentwriters > 0

    @propertycache
    def _map(self):
        """Return the dirstate contents (see documentation for dirstatemap)."""
        self._map = self._mapcls(self._ui, self._opener, self._root)
        return self._map

    @repocache("branch")
    def _branch(self):
        try:
            return self._opener.read("branch").strip() or "default"
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            return "default"

    @property
    def _pl(self):
        return self._map.parents()

    def hasdir(self, d):
        return self._map.hastrackeddir(d)

    @rootcache(".hgignore")
    def _ignore(self):
        # gitignore
        globalignores = self._globalignorefiles()
        return matchmod.gitignorematcher(self._root, "", gitignorepaths=globalignores)

    @propertycache
    def _slash(self):
        return (
            self._ui.plain() or self._ui.configbool("ui", "slash")
        ) and pycompat.ossep != "/"

    @propertycache
    def _checklink(self):
        return util.checklink(self._root)

    @propertycache
    def _checkexec(self):
        return util.checkexec(self._root)

    @propertycache
    def _checkcase(self):
        return not util.fscasesensitive(self._join(".hg"))

    def _join(self, f):
        # much faster than os.path.join()
        # it's safe because f is always a relative path
        return self._rootdir + f

    def flagfunc(self, buildfallback):
        if self._checklink and self._checkexec:

            def f(x):
                try:
                    st = os.lstat(self._join(x))
                    if util.statislink(st):
                        return "l"
                    if util.statisexec(st):
                        return "x"
                except OSError:
                    pass
                return ""

            return f

        fallback = buildfallback()
        if self._checklink:

            def f(x):
                if os.path.islink(self._join(x)):
                    return "l"
                if "x" in fallback(x):
                    return "x"
                return ""

            return f
        if self._checkexec:

            def f(x):
                if "l" in fallback(x):
                    return "l"
                if util.isexec(self._join(x)):
                    return "x"
                return ""

            return f
        else:
            return fallback

    @propertycache
    def _cwd(self):
        # internal config: ui.forcecwd
        forcecwd = self._ui.config("ui", "forcecwd")
        if forcecwd:
            return forcecwd
        return pycompat.getcwd()

    def getcwd(self):
        """Return the path from which a canonical path is calculated.

        This path should be used to resolve file patterns or to convert
        canonical paths back to file paths for display. It shouldn't be
        used to get real file paths. Use vfs functions instead.
        """
        cwd = self._cwd
        if cwd == self._root:
            return ""
        # self._root ends with a path separator if self._root is '/' or 'C:\'
        rootsep = self._root
        if not util.endswithsep(rootsep):
            rootsep += pycompat.ossep
        if cwd.startswith(rootsep):
            return cwd[len(rootsep) :]
        else:
            # we're outside the repo. return an absolute path.
            return cwd

    def pathto(self, f, cwd=None):
        if cwd is None:
            cwd = self.getcwd()
        path = util.pathto(self._root, cwd, f)
        if self._slash:
            return util.pconvert(path)
        return path

    def __getitem__(self, key):
        """Return the current state of key (a filename) in the dirstate.

        States are:
          n  normal
          m  needs merging
          r  marked for removal
          a  marked for addition
          ?  not tracked
        """
        return self._map.get(key, ("?",))[0]

    def __contains__(self, key):
        return key in self._map

    def __iter__(self):
        return iter(sorted(self._map))

    def items(self):
        return self._map.iteritems()

    iteritems = items

    def parents(self):
        return [self._validate(p) for p in self._pl]

    def p1(self):
        return self._validate(self._pl[0])

    def p2(self):
        return self._validate(self._pl[1])

    def branch(self):
        return encoding.tolocal(self._branch)

    def setparents(self, p1, p2=nullid):
        """Set dirstate parents to p1 and p2.

        When moving from two parents to one, 'm' merged entries a
        adjusted to normal and previous copy records discarded and
        returned by the call.

        See localrepo.setparents()
        """
        if self._parentwriters == 0:
            raise ValueError(
                "cannot set dirstate parent without "
                "calling dirstate.beginparentchange"
            )

        self._dirty = True
        oldp2 = self._pl[1]
        if self._origpl is None:
            self._origpl = self._pl
        self._map.setparents(p1, p2)
        copies = {}
        copymap = self._map.copymap
        if oldp2 != nullid and p2 == nullid:
            candidatefiles = self._map.nonnormalset.union(self._map.otherparentset)
            for f in candidatefiles:
                s = self._map.get(f)
                if s is None:
                    continue

                # Discard 'm' markers when moving away from a merge state
                if s[0] == "m":
                    source = copymap.get(f)
                    if source:
                        copies[f] = source
                    self.normallookup(f)
                # Also fix up otherparent markers
                elif s[0] == "n" and s[2] == -2:
                    source = copymap.get(f)
                    if source:
                        copies[f] = source
                    self.add(f)
        return copies

    def setbranch(self, branch):
        self._branch = encoding.fromlocal(branch)
        f = self._opener("branch", "w", atomictemp=True, checkambig=True)
        try:
            f.write(self._branch + "\n")
            f.close()

            # make sure filecache has the correct stat info for _branch after
            # replacing the underlying file
            ce = self._filecache["_branch"]
            if ce:
                ce.refresh()
        except:  # re-raises
            f.discard()
            raise

    def invalidate(self):
        """Causes the next access to reread the dirstate.

        This is different from localrepo.invalidatedirstate() because it always
        rereads the dirstate. Use localrepo.invalidatedirstate() if you want to
        check whether the dirstate has changed before rereading it."""

        for a in ("_map", "_branch", "_ignore"):
            if a in self.__dict__:
                delattr(self, a)
        self._lastnormaltime = 0
        self._dirty = False
        self._updatedfiles.clear()
        self._parentwriters = 0
        self._origpl = None

    def copy(self, source, dest):
        """Mark dest as a copy of source. Unmark dest if source is None."""
        if source == dest:
            return
        self._dirty = True
        if self._istreestate:
            self._map.copy(source, dest)
            # treestatemap.copymap needs to be changed via the "copy" method.
            # _updatedfiles is not used by treestatemap as it's tracked
            # internally.
            return
        if source is not None:
            self._map.copymap[dest] = source
            self._updatedfiles.add(source)
            self._updatedfiles.add(dest)
        elif self._map.copymap.pop(dest, None):
            self._updatedfiles.add(dest)

    def copied(self, file):
        if self._istreestate:
            return self._map.copysource(file)
        else:
            return self._map.copymap.get(file, None)

    def copies(self):
        return self._map.copymap

    def needcheck(self, file):
        """Mark file as need-check"""
        if not self._istreestate:
            raise error.ProgrammingError("needcheck is only supported by treestate")
        changed = self._map.needcheck(file)
        self._dirty |= changed
        return changed

    def clearneedcheck(self, file):
        if not self._istreestate:
            raise error.ProgrammingError("needcheck is only supported by treestate")
        changed = self._map.clearneedcheck(file)
        self._dirty |= changed

    def setclock(self, clock):
        """Set fsmonitor clock"""
        return self.setmeta("clock", clock)

    def getclock(self):
        """Get fsmonitor clock"""
        return self.getmeta("clock")

    def setmeta(self, name, value):
        """Set metadata"""
        if not self._istreestate:
            raise error.ProgrammingError("setmeta is only supported by treestate")
        value = value or None
        if value != self.getmeta(name):
            self._map.updatemetadata({name: value})
            self._dirty = True

    def getmeta(self, name):
        """Get metadata"""
        if not self._istreestate:
            raise error.ProgrammingError("getmeta is only supported by treestate")
        # Normalize "" to "None"
        return self._map.getmetadata().get(name) or None

    def _addpath(self, f, state, mode, size, mtime):
        oldstate = self[f]
        if state == "a" or oldstate == "r":
            scmutil.checkfilename(f)
            if self._map.hastrackeddir(f):
                raise error.Abort(_("directory %r already in dirstate") % f)
            # shadows
            for d in util.finddirs(f):
                if self._map.hastrackeddir(d):
                    break
                entry = self._map.get(d)
                if entry is not None and entry[0] not in "r?":
                    raise error.Abort(_("file %r in dirstate clashes with %r") % (d, f))
        self._dirty = True
        self._updatedfiles.add(f)
        self._map.addfile(f, oldstate, state, mode, size, mtime)

    def normal(self, f):
        """Mark a file normal and clean."""
        s = os.lstat(self._join(f))
        mtime = s.st_mtime
        self._addpath(f, "n", s.st_mode, s.st_size & _rangemask, mtime & _rangemask)
        if not self._istreestate:
            self._map.copymap.pop(f, None)
            if f in self._map.nonnormalset:
                self._map.nonnormalset.remove(f)
        if mtime > self._lastnormaltime:
            # Remember the most recent modification timeslot for status(),
            # to make sure we won't miss future size-preserving file content
            # modifications that happen within the same timeslot.
            self._lastnormaltime = mtime

    def normallookup(self, f):
        """Mark a file normal, but possibly dirty."""
        if self._pl[1] != nullid:
            # if there is a merge going on and the file was either
            # in state 'm' (-1) or coming from other parent (-2) before
            # being removed, restore that state.
            entry = self._map.get(f)
            if entry is not None:
                if entry[0] == "r" and entry[2] in (-1, -2):
                    source = self._map.copymap.get(f)
                    if entry[2] == -1:
                        self.merge(f)
                    elif entry[2] == -2:
                        self.otherparent(f)
                    if source:
                        self.copy(source, f)
                    return
                if entry[0] == "m" or entry[0] == "n" and entry[2] == -2:
                    return
        self._addpath(f, "n", 0, -1, -1)
        if not self._istreestate:
            self._map.copymap.pop(f, None)

    def otherparent(self, f):
        """Mark as coming from the other parent, always dirty."""
        if self._pl[1] == nullid:
            raise error.Abort(
                _("setting %r to other parent " "only allowed in merges") % f
            )
        if f in self and self[f] == "n":
            # merge-like
            self._addpath(f, "m", 0, -2, -1)
        else:
            # add-like
            self._addpath(f, "n", 0, -2, -1)
        if not self._istreestate:
            self._map.copymap.pop(f, None)

    def add(self, f):
        """Mark a file added."""
        self._addpath(f, "a", 0, -1, -1)
        if not self._istreestate:
            self._map.copymap.pop(f, None)

    def remove(self, f):
        """Mark a file removed."""
        self._dirty = True
        oldstate = self[f]
        size = 0
        if self._pl[1] != nullid:
            entry = self._map.get(f)
            if entry is not None:
                # backup the previous state
                if entry[0] == "m":  # merge
                    size = -1
                elif entry[0] == "n" and entry[2] == -2:  # other parent
                    size = -2
                    if not self._istreestate:
                        self._map.otherparentset.add(f)
        self._updatedfiles.add(f)
        self._map.removefile(f, oldstate, size)
        if not self._istreestate:
            if size == 0:
                self._map.copymap.pop(f, None)

    def merge(self, f):
        """Mark a file merged."""
        if self._pl[1] == nullid:
            return self.normallookup(f)
        return self.otherparent(f)

    def untrack(self, f):
        """Stops tracking a file in the dirstate. This is useful during
        operations that want to stop tracking a file, but still have it show up
        as untracked (like hg forget)."""
        oldstate = self[f]
        if self._map.untrackfile(f, oldstate):
            self._dirty = True
            if not self._istreestate:
                self._updatedfiles.add(f)
                self._map.copymap.pop(f, None)

    def delete(self, f):
        """Removes a file from the dirstate entirely. This is useful during
        operations like update, to remove files from the dirstate that are known
        to be deleted."""
        oldstate = self[f]
        if self._map.deletefile(f, oldstate):
            self._dirty = True
            if not self._istreestate:
                self._updatedfiles.add(f)
                self._map.copymap.pop(f, None)

    def _discoverpath(self, path, normed, ignoremissing, exists, storemap):
        if exists is None:
            exists = os.path.lexists(os.path.join(self._root, path))
        if not exists:
            # Maybe a path component exists
            if not ignoremissing and "/" in path:
                d, f = path.rsplit("/", 1)
                d = self._normalize(d, False, ignoremissing, None)
                folded = d + "/" + f
            else:
                # No path components, preserve original case
                folded = path
        else:
            # recursively normalize leading directory components
            # against dirstate
            if "/" in normed:
                d, f = normed.rsplit("/", 1)
                d = self._normalize(d, False, ignoremissing, True)
                r = self._root + "/" + d
                folded = d + "/" + util.fspath(f, r)
            else:
                folded = util.fspath(normed, self._root)
            storemap[normed] = folded

        return folded

    def _normalizefile(self, path, isknown, ignoremissing=False, exists=None):
        normed = util.normcase(path)
        folded = self._map.filefoldmap.get(normed, None)
        if folded is None:
            if isknown:
                folded = path
            else:
                folded = self._discoverpath(
                    path, normed, ignoremissing, exists, self._map.filefoldmap
                )
        return folded

    def _normalize(self, path, isknown, ignoremissing=False, exists=None):
        normed = util.normcase(path)
        folded = self._map.filefoldmap.get(normed, None)
        if folded is None:
            folded = self._map.dirfoldmap.get(normed, None)
        if folded is None:
            if isknown:
                folded = path
            else:
                # store discovered result in dirfoldmap so that future
                # normalizefile calls don't start matching directories
                folded = self._discoverpath(
                    path, normed, ignoremissing, exists, self._map.dirfoldmap
                )
        return folded

    def normalize(self, path, isknown=False, ignoremissing=False):
        """
        normalize the case of a pathname when on a casefolding filesystem

        isknown specifies whether the filename came from walking the
        disk, to avoid extra filesystem access.

        If ignoremissing is True, missing path are returned
        unchanged. Otherwise, we try harder to normalize possibly
        existing path components.

        The normalized case is determined based on the following precedence:

        - version of name already stored in the dirstate
        - version of name stored on disk
        - version provided via command arguments
        """

        if self._checkcase:
            return self._normalize(path, isknown, ignoremissing)
        return path

    def clear(self):
        self._map.clear()
        self._lastnormaltime = 0
        self._updatedfiles.clear()
        self._dirty = True

    def rebuild(self, parent, allfiles, changedfiles=None, exact=False):
        # If exact is True, then assume only changedfiles can be changed, and
        # other files cannot be possibly changed. This is used by "absorb" as
        # a hint to perform a fast path for fsmonitor and sparse.
        if changedfiles is None:
            if exact:
                raise error.ProgrammingError("exact requires changedfiles")
            # Rebuild entire dirstate
            changedfiles = allfiles
            lastnormaltime = self._lastnormaltime
            self.clear()
            self._lastnormaltime = lastnormaltime

        if self._origpl is None:
            self._origpl = self._pl
        self._map.setparents(parent, nullid)
        for f in changedfiles:
            if f in allfiles:
                self.normallookup(f)
            else:
                self.untrack(f)

        self._dirty = True

    def identity(self):
        """Return identity of dirstate itself to detect changing in storage

        If identity of previous dirstate is equal to this, writing
        changes based on the former dirstate out can keep consistency.
        """
        return self._map.identity

    def write(self, tr):
        if not self._dirty:
            return

        filename = self._filename
        if tr:
            # 'dirstate.write()' is not only for writing in-memory
            # changes out, but also for dropping ambiguous timestamp.
            # delayed writing re-raise "ambiguous timestamp issue".
            # See also the wiki page below for detail:
            # https://www.mercurial-scm.org/wiki/DirstateTransactionPlan

            # emulate dropping timestamp in 'parsers.pack_dirstate'
            now = _getfsnow(self._opener)
            self._map.clearambiguoustimes(self._updatedfiles, now)

            # emulate that all 'dirstate.normal' results are written out
            self._lastnormaltime = 0
            self._updatedfiles.clear()

            # delay writing in-memory changes out
            tr.addfilegenerator(
                "dirstate", (self._filename,), self._writedirstate, location="local"
            )
            return

        st = self._opener(filename, "w", atomictemp=True, checkambig=True)
        self._writedirstate(st)

    @util.propertycache
    def checkoutidentifier(self):
        try:
            return self._opener.read("checkoutidentifier")
        except IOError as e:
            if e.errno != errno.ENOENT:
                raise
        return ""

    def addparentchangecallback(self, category, callback):
        """add a callback to be called when the wd parents are changed

        Callback will be called with the following arguments:
            dirstate, (oldp1, oldp2), (newp1, newp2)

        Category is a unique identifier to allow overwriting an old callback
        with a newer callback.
        """
        self._plchangecallbacks[category] = callback

    def _writedirstate(self, st):
        # notify callbacks about parents change
        if self._origpl is not None and self._origpl != self._pl:
            for c, callback in sorted(self._plchangecallbacks.iteritems()):
                callback(self, self._origpl, self._pl)
            # if the first parent has changed then consider this a new checkout
            if self._origpl[0] != self._pl[0]:
                with self._opener("checkoutidentifier", "w", atomictemp=True) as f:
                    f.write(util.makerandomidentifier())
                util.clearcachedproperty(self, "checkoutidentifier")
            self._origpl = None
        # use the modification time of the newly created temporary file as the
        # filesystem's notion of 'now'
        now = util.fstat(st).st_mtime & _rangemask

        # enough 'delaywrite' prevents 'pack_dirstate' from dropping
        # timestamp of each entries in dirstate, because of 'now > mtime'
        delaywrite = self._ui.configint("debug", "dirstate.delaywrite")
        if delaywrite > 0:
            # do we have any files to delay for?
            for f, e in self._map.iteritems():
                if e[0] == "n" and e[3] == now:
                    import time  # to avoid useless import

                    # rather than sleep n seconds, sleep until the next
                    # multiple of n seconds
                    clock = time.time()
                    start = int(clock) - (int(clock) % delaywrite)
                    end = start + delaywrite
                    time.sleep(end - clock)
                    now = end  # trust our estimate that the end is near now
                    break

        self._map.write(st, now)
        self._lastnormaltime = 0
        self._dirty = False

    def _dirignore(self, f):
        if f == "":
            return False
        visitdir = self._ignore.visitdir
        if visitdir(f) == "all":
            return True
        return False

    def _ignorefiles(self):
        files = []
        files += self._globalignorefiles()
        return files

    def _globalignorefiles(self):
        files = []
        for name, path in self._ui.configitems("ui"):
            # A path could have an optional prefix (ex. "git:") to select file
            # format
            if name == "ignore" or name.startswith("ignore."):
                # we need to use os.path.join here rather than self._join
                # because path is arbitrary and user-specified
                fullpath = os.path.join(self._rootdir, util.expandpath(path))
                files.append(fullpath)
        return files

    @perftrace.tracefunc("Status")
    def status(self, match, ignored, clean, unknown):
        """Determine the status of the working copy relative to the
        dirstate and return a pair of (unsure, status), where status is of type
        scmutil.status and:

          unsure:
            files that might have been modified since the dirstate was
            written, but need to be read to be sure (size is the same
            but mtime differs)
          status.modified:
            files that have definitely been modified since the dirstate
            was written (different size or mode)
          status.clean:
            files that have definitely not been modified since the
            dirstate was written
        """
        wctx = self._repo[None]
        # Prime the wctx._parents cache so the parent doesn't change out from
        # under us if a checkout happens in another process.
        pctx = wctx.parents()[0]

        listignored, listclean, listunknown = ignored, clean, unknown
        lookup, modified, added, unknown, ignored = set(), [], [], [], []
        removed, deleted, clean = [], [], []

        dmap = self._map
        dmap.preload()
        dget = dmap.__getitem__
        ladd = lookup.add  # aka "unsure"
        madd = modified.append
        aadd = added.append
        uadd = unknown.append
        iadd = ignored.append
        radd = removed.append
        dadd = deleted.append
        cadd = clean.append
        ignore = self._ignore
        copymap = self._map.copymap

        # We have seen some rare issues that a few "M" or "R" files show up
        # while the files are expected to be clean. Log the reason of first few
        # "M" files.
        mtolog = self._ui.configint("experimental", "samplestatus")

        nonnormalset = dmap.nonnormalset
        otherparentset = dmap.otherparentset

        # Step 1: Get the files that are different from the clean checkedout p1 tree.
        pendingchanges = self._fs.pendingchanges(match, listignored=listignored)

        for fn, exists, needslookup in pendingchanges:
            try:
                t = dget(fn)
                # This "?" state is only tracked by treestate, emulate the old
                # behavior - KeyError.
                if t[0] == "?":
                    raise KeyError
            except KeyError:
                isignored = ignore(fn)
                if listignored and isignored:
                    iadd(fn)
                elif listunknown and not isignored:
                    uadd(fn)
                continue

            state = t[0]
            if not exists and state in "nma":
                dadd(fn)
            elif state == "n":
                # Lookup handling is temporary until fs.pendingchanges can
                # handle it.
                if needslookup:
                    ladd(fn)
                else:
                    madd(fn)
            else:
                # All other states will be handled by the logic below, and we
                # don't care that it's a pending change.
                pass

        # The seen set is used to prevent steps 2 and 3 from processing things
        # we saw in step 1. We explicitly do not add lookup to the seen set,
        # because that would prevent them from being processed in Step 2. It's
        # possible step 2 would classify something as modified, while lookup
        # would classify it as clean, so let's give step 2 a chance, then remove
        # things from lookup that were processed in step 2.
        seenset = set(deleted + modified)

        # Step 2: Handle status results that are not simply pending filesystem
        # changes on top of the pristine tree.
        for fn in otherparentset:
            if not match(fn) or fn in seenset:
                continue
            t = dget(fn)
            state = t[0]
            # We only need to handle 'n' here, since all other states will be
            # covered by the nonnormal loop below.
            if state in "n":
                try:
                    # pendingchanges() above only checks for changes against p1.
                    # For things from p2, we need to manually check for
                    # existence. We don't have to check if they're modified,
                    # since them coming from p2 indicates they are considered
                    # modified.
                    os.lstat(self._join(fn))
                    if mtolog > 0:
                        mtolog -= 1
                        self._ui.log("status", "M %s: exists in p2" % fn)
                    madd(fn)
                except OSError:
                    dadd(fn)
                seenset.add(fn)

        for fn in nonnormalset:
            if not match(fn) or fn in seenset:
                continue
            t = dget(fn)
            state = t[0]
            if state == "m":
                madd(fn)
                seenset.add(fn)
                if mtolog > 0:
                    mtolog -= 1
                    self._ui.log("status", "M %s: state is 'm' (merge)" % fn)
            elif state == "a":
                aadd(fn)
                seenset.add(fn)
            elif state == "r":
                radd(fn)
                seenset.add(fn)
            elif state == "n":
                # This can happen if the file is in a lookup state, but all 'n'
                # files should've been checked in fs.pendingchanges, so we can
                # ignore it here.
                pass
            elif state == "?":
                # I'm pretty sure this is a bug if nonnormalset contains unknown
                # files, but the tests say it can happen so let's just ignore
                # it.
                pass
            else:
                raise error.ProgrammingError(
                    "unexpected nonnormal state '%s' " "for '%s'" % (t, fn)
                )

        # Most copies should be handled above, as modifies or adds, but there
        # can be cases where a file is clean and already committed and a commit
        # is just retroactively marking it as copied. In that case we need to
        # mark is as modified.
        for fn in copymap:
            if not match(fn) or fn in seenset:
                continue
            # It seems like a bug, but the tests show that copymap can contain
            # files that aren't in the dirstate. I believe this is caused by
            # using treestate, which leaves the copymap as partially maintained.
            if fn not in dmap:
                continue
            madd(fn)
            seenset.add(fn)

        status = scmutil.status(
            modified, added, removed, deleted, unknown, ignored, clean
        )

        fixup = []
        if lookup:
            lookup = lookup - seenset
            modified2, deleted2, fixup = self._checklookup(wctx, lookup)
            status.modified.extend(modified2)
            status.deleted.extend(deleted2)

            if fixup and listclean:
                status.clean.extend(fixup)

        # Step 3: If clean files were requested, add those to the results
        seenset = set()
        for files in status:
            seenset.update(files)
            seenset.update(util.dirs(files))

        if listclean:
            for fn in pctx.manifest().matches(match):
                if fn not in seenset:
                    cadd(fn)
            seenset.update(clean)

        # Step 4: Report any explicitly requested files that don't exist
        for path in sorted(match.files()):
            try:
                if path in seenset:
                    continue
                os.lstat(os.path.join(self._root, path))
            except OSError as ex:
                match.bad(path, encoding.strtolocal(ex.strerror))

        if not getattr(self._repo, "_insidepoststatusfixup", False):
            self._poststatusfixup(status, fixup, wctx)

        perftrace.tracevalue("A/M/R Files", len(modified) + len(added) + len(removed))
        if len(unknown) > 0:
            perftrace.tracevalue("Unknown Files", len(unknown))
        if len(ignored) > 0:
            perftrace.tracevalue("Ignored Files", len(ignored))
        return status

    def _checklookup(self, wctx, files):
        # check for any possibly clean files
        if not files:
            return [], [], []

        modified = []
        deleted = []
        fixup = []
        pctx = wctx.parents()[0]

        # Log some samples
        ui = self._ui
        mtolog = ftolog = dtolog = ui.configint("experimental", "samplestatus")

        # do a full compare of any files that might have changed
        for f in sorted(files):
            try:
                # This will return True for a file that got replaced by a
                # directory in the interim, but fixing that is pretty hard.
                if (
                    f not in pctx
                    or wctx.flags(f) != pctx.flags(f)
                    or pctx[f].cmp(wctx[f])
                ):
                    modified.append(f)
                    if mtolog > 0:
                        mtolog -= 1
                        ui.log("status", "M %s: checked in filesystem" % f)
                else:
                    fixup.append(f)
                    if ftolog > 0:
                        ftolog -= 1
                        ui.log("status", "C %s: checked in filesystem" % f)
            except (IOError, OSError):
                # A file become inaccessible in between? Mark it as deleted,
                # matching dirstate behavior (issue5584).
                # The dirstate has more complex behavior around whether a
                # missing file matches a directory, etc, but we don't need to
                # bother with that: if f has made it to this point, we're sure
                # it's in the dirstate.
                deleted.append(f)
                if dtolog > 0:
                    dtolog -= 1
                    ui.log("status", "R %s: checked in filesystem" % f)

        return modified, deleted, fixup

    def _poststatusfixup(self, status, fixup, wctx):
        """update dirstate for files that are actually clean"""
        poststatusbefore = self._repo.postdsstatus(afterdirstatewrite=False)
        poststatusafter = self._repo.postdsstatus(afterdirstatewrite=True)
        ui = self._repo.ui
        if fixup or poststatusbefore or poststatusafter or self._dirty:
            # prevent infinite loop because fsmonitor postfixup might call
            # wctx.status()
            self._repo._insidepoststatusfixup = True
            try:
                oldid = self.identity()

                # Updating the dirstate is optional so we don't wait on the
                # lock.
                # wlock can invalidate the dirstate, so cache normal _after_
                # taking the lock. This is a bit weird because we're inside the
                # dirstate that is no longer valid.

                # If watchman reports fresh instance, still take the lock,
                # since not updating watchman state leads to very painful
                # performance.
                freshinstance = False
                try:
                    freshinstance = self._fs._fsmonitorstate._lastisfresh
                except Exception:
                    pass
                if freshinstance:
                    ui.debug(
                        "poststatusfixup decides to wait for wlock since watchman reported fresh instance\n"
                    )

                with self._repo.wlock(freshinstance):
                    if self._repo.dirstate.identity() == oldid:
                        if poststatusbefore:
                            for ps in poststatusbefore:
                                ps(wctx, status)

                        if fixup:
                            normal = self.normal
                            for f in fixup:
                                normal(f)

                        # write changes out explicitly, because nesting
                        # wlock at runtime may prevent 'wlock.release()'
                        # after this block from doing so for subsequent
                        # changing files
                        #
                        # This is a no-op if dirstate is not dirty.
                        tr = self._repo.currenttransaction()
                        self.write(tr)

                        if poststatusafter:
                            for ps in poststatusafter:
                                ps(wctx, status)
                    else:
                        if freshinstance:
                            ui.write_err(
                                _(
                                    "warning: failed to update watchman state because dirstate has been changed by other processes\n"
                                )
                            )
                            ui.write_err(slowstatuswarning)

                        # in this case, writing changes out breaks
                        # consistency, because .hg/dirstate was
                        # already changed simultaneously after last
                        # caching (see also issue5584 for detail)
                        self._repo.ui.debug(
                            "skip updating dirstate: " "identity mismatch\n"
                        )
            except error.LockError:
                if freshinstance:
                    ui.write_err(
                        _(
                            "warning: failed to update watchman state because wlock cannot be obtained\n"
                        )
                    )
                    ui.write_err(slowstatuswarning)
            finally:
                # Even if the wlock couldn't be grabbed, clear out the list.
                self._repo.clearpostdsstatus()
                self._repo._insidepoststatusfixup = False

    def matches(self, match):
        """
        return files in the dirstate (in whatever state) filtered by match
        """
        dmap = self._map
        if match.always():
            return dmap.keys()
        files = match.files()
        if match.isexact():
            # fast path -- filter the other way around, since typically files is
            # much smaller than dmap
            return [f for f in files if f in dmap]
        if match.prefix():
            if self._istreestate:
                # treestate has a fast path to get files inside a subdirectory.
                # files are prefixes
                result = set()
                fastpathvalid = True
                for prefix in files:
                    if prefix in dmap:
                        # prefix is a file
                        result.add(prefix)
                    elif dmap.hastrackeddir(prefix + "/"):
                        # prefix is a directory
                        result.update(dmap.keys(prefix=prefix + "/"))
                    else:
                        # unknown pattern (ex. "."), fast path is invalid
                        fastpathvalid = False
                        break
                if fastpathvalid:
                    return sorted(result)
            else:
                # fast path -- all the values are known to be files, so just
                # return that
                if all(fn in dmap for fn in files):
                    return list(files)
        return [f for f in dmap if match(f)]

    def _actualfilename(self, tr):
        if tr:
            return self._pendingfilename
        else:
            return self._filename

    def savebackup(self, tr, backupname):
        """Save current dirstate into backup file"""
        filename = self._actualfilename(tr)
        assert backupname != filename

        # use '_writedirstate' instead of 'write' to write changes certainly,
        # because the latter omits writing out if transaction is running.
        # output file will be used to create backup of dirstate at this point.
        if self._dirty or not self._opener.exists(filename):
            self._writedirstate(
                self._opener(filename, "w", atomictemp=True, checkambig=True)
            )

        if tr:
            # ensure that subsequent tr.writepending returns True for
            # changes written out above, even if dirstate is never
            # changed after this
            tr.addfilegenerator(
                "dirstate", (self._filename,), self._writedirstate, location="local"
            )

            # ensure that pending file written above is unlinked at
            # failure, even if tr.writepending isn't invoked until the
            # end of this transaction
            tr.registertmp(filename, location="local")

        self._opener.tryunlink(backupname)
        # hardlink backup is okay because _writedirstate is always called
        # with an "atomictemp=True" file.
        util.copyfile(
            self._opener.join(filename), self._opener.join(backupname), hardlink=True
        )

    def restorebackup(self, tr, backupname):
        """Restore dirstate by backup file"""
        # this "invalidate()" prevents "wlock.release()" from writing
        # changes of dirstate out after restoring from backup file
        self.invalidate()
        filename = self._actualfilename(tr)
        o = self._opener
        if util.samefile(o.join(backupname), o.join(filename)):
            o.unlink(backupname)
        else:
            o.rename(backupname, filename, checkambig=True)

    def clearbackup(self, tr, backupname):
        """Clear backup file"""
        self._opener.unlink(backupname)

    def loginfo(self, ui, prefix):
        try:
            parents = [hex(p) if p != nullid else "" for p in self._pl]
        except Exception:
            # The dirstate may be too corrupt to read.  We don't want to fail
            # just because of logging, so log the parents as unknown.
            parents = ("unknown", "unknown")
        data = {
            prefix + "checkoutidentifier": self.checkoutidentifier,
            prefix + "wdirparent1": parents[0],
            prefix + "wdirparent2": parents[1],
        }
        ui.log("dirstate_info", **data)


class dirstatemap(object):
    """Map encapsulating the dirstate's contents.

    The dirstate contains the following state:

    - `identity` is the identity of the dirstate file, which can be used to
      detect when changes have occurred to the dirstate file.

    - `parents` is a pair containing the parents of the working copy. The
      parents are updated by calling `setparents`.

    - the state map maps filenames to tuples of (state, mode, size, mtime),
      where state is a single character representing 'normal', 'added',
      'removed', or 'merged'. It is read by treating the dirstate as a
      dict.  File state is updated by calling the `addfile`, `removefile` and
      `untrackfile` methods.

    - `copymap` maps destination filenames to their source filename.

    The dirstate also provides the following views onto the state:

    - `nonnormalset` is a set of the filenames that have state other
      than 'normal', or are normal but have an mtime of -1 ('normallookup').

    - `otherparentset` is a set of the filenames that are marked as coming
      from the second parent when the dirstate is currently being merged.

    - `filefoldmap` is a dict mapping normalized filenames to the denormalized
      form that they appear as in the dirstate.

    - `dirfoldmap` is a dict mapping normalized directory names to the
      denormalized form that they appear as in the dirstate.
    """

    def __init__(self, ui, opener, root):
        self._ui = ui
        self._opener = opener
        self._root = root
        self._filename = "dirstate"

        self._parents = None
        self._dirtyparents = False

        # for consistent view between _pl() and _read() invocations
        self._pendingmode = None

    @propertycache
    def _map(self):
        self._map = {}
        self.read()
        return self._map

    @propertycache
    def copymap(self):
        self.copymap = {}
        self._map
        return self.copymap

    def clear(self):
        self._map.clear()
        self.copymap.clear()
        self.setparents(nullid, nullid)
        util.clearcachedproperty(self, "_dirs")
        util.clearcachedproperty(self, "_alldirs")
        util.clearcachedproperty(self, "filefoldmap")
        util.clearcachedproperty(self, "dirfoldmap")
        util.clearcachedproperty(self, "nonnormalset")
        util.clearcachedproperty(self, "otherparentset")

    def iteritems(self):
        return self._map.iteritems()

    def __len__(self):
        return len(self._map)

    def __iter__(self):
        return iter(self._map)

    def get(self, key, default=None):
        return self._map.get(key, default)

    def __contains__(self, key):
        return key in self._map

    def __getitem__(self, key):
        return self._map[key]

    def keys(self):
        return self._map.keys()

    def preload(self):
        """Loads the underlying data, if it's not already loaded"""
        self._map

    def addfile(self, f, oldstate, state, mode, size, mtime):
        """Add a tracked file to the dirstate."""
        if oldstate in "?r" and "_dirs" in self.__dict__:
            self._dirs.addpath(f)
        if oldstate == "?" and "_alldirs" in self.__dict__:
            self._alldirs.addpath(f)
        self._insert_tuple(f, state, mode, size, mtime)
        if state != "n" or mtime == -1:
            self.nonnormalset.add(f)
        if size == -2:
            self.otherparentset.add(f)

    def removefile(self, f, oldstate, size):
        """
        Mark a file as removed in the dirstate.

        The `size` parameter is used to store sentinel values that indicate
        the file's previous state.  In the future, we should refactor this
        to be more explicit about what that state is.
        """
        if oldstate not in "?r" and "_dirs" in self.__dict__:
            self._dirs.delpath(f)
        if oldstate == "?" and "_alldirs" in self.__dict__:
            self._alldirs.addpath(f)
        if "filefoldmap" in self.__dict__:
            normed = util.normcase(f)
            self.filefoldmap.pop(normed, None)
        self._insert_tuple(f, "r", 0, size, 0)
        self.nonnormalset.add(f)

    def deletefile(self, f, oldstat):
        """
        Removes a file from the dirstate entirely, implying it doesn't even
        exist on disk anymore and may not be untracked.
        """
        # In the default dirstate implementation, deletefile is the same as
        # untrackfile.
        self.untrackfile(f, oldstat)

    def untrackfile(self, f, oldstate):
        """
        Remove a file from the dirstate, leaving it untracked.  Returns True if
        the file was previously recorded.
        """
        exists = self._map.pop(f, None) is not None
        if exists:
            if oldstate != "r" and "_dirs" in self.__dict__:
                self._dirs.delpath(f)
            if "_alldirs" in self.__dict__:
                self._alldirs.delpath(f)
        if "filefoldmap" in self.__dict__:
            normed = util.normcase(f)
            self.filefoldmap.pop(normed, None)
        self.nonnormalset.discard(f)
        return exists

    def clearambiguoustimes(self, files, now):
        for f in files:
            e = self.get(f)
            if e is not None and e[0] == "n" and e[3] == now:
                self._insert_tuple(f, e[0], e[1], e[2], -1)
                self.nonnormalset.add(f)

    def _insert_tuple(self, f, state, mode, size, mtime):
        self._map[f] = dirstatetuple(state, mode, size, mtime)

    def nonnormalentries(self):
        """Compute the nonnormal dirstate entries from the dmap"""
        try:
            return parsers.nonnormalotherparententries(self._map)
        except AttributeError:
            nonnorm = set()
            otherparent = set()
            for fname, e in self._map.iteritems():
                if e[0] != "n" or e[3] == -1:
                    nonnorm.add(fname)
                if e[0] == "n" and e[2] == -2:
                    otherparent.add(fname)
            return nonnorm, otherparent

    @propertycache
    def filefoldmap(self):
        """Returns a dictionary mapping normalized case paths to their
        non-normalized versions.
        """
        try:
            makefilefoldmap = parsers.make_file_foldmap
        except AttributeError:
            pass
        else:
            return makefilefoldmap(self._map, util.normcasespec, util.normcasefallback)

        f = {}
        normcase = util.normcase
        for name, s in self._map.iteritems():
            if s[0] != "r":
                f[normcase(name)] = name
        f["."] = "."  # prevents useless util.fspath() invocation
        return f

    def hastrackeddir(self, d):
        """
        Returns True if the dirstate contains a tracked (not removed) file
        in this directory.
        """
        return d in self._dirs

    def hasdir(self, d):
        """
        Returns True if the dirstate contains a file (tracked or removed)
        in this directory.
        """
        return d in self._alldirs

    @propertycache
    def _dirs(self):
        return util.dirs(self._map, "r")

    @propertycache
    def _alldirs(self):
        return util.dirs(self._map)

    def _opendirstatefile(self):
        fp, mode = txnutil.trypending(self._root, self._opener, self._filename)
        if self._pendingmode is not None and self._pendingmode != mode:
            fp.close()
            raise error.Abort(_("working directory state may be " "changed parallelly"))
        self._pendingmode = mode
        return fp

    def parents(self):
        if not self._parents:
            try:
                fp = self._opendirstatefile()
                st = fp.read(40)
                fp.close()
            except IOError as err:
                if err.errno != errno.ENOENT:
                    raise
                # File doesn't exist, so the current state is empty
                st = ""

            l = len(st)
            if l == 40:
                self._parents = st[:20], st[20:40]
            elif l == 0:
                self._parents = [nullid, nullid]
            else:
                raise error.Abort(_("working directory state appears " "damaged!"))

        return self._parents

    def setparents(self, p1, p2):
        self._parents = (p1, p2)
        self._dirtyparents = True

    def read(self):
        # ignore HG_PENDING because identity is used only for writing
        self.identity = util.filestat.frompath(self._opener.join(self._filename))

        try:
            fp = self._opendirstatefile()
            try:
                st = fp.read()
            finally:
                fp.close()
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            return
        if not st:
            return

        if util.safehasattr(parsers, "dict_new_presized"):
            # Make an estimate of the number of files in the dirstate based on
            # its size. From a linear regression on a set of real-world repos,
            # all over 10,000 files, the size of a dirstate entry is 85
            # bytes. The cost of resizing is significantly higher than the cost
            # of filling in a larger presized dict, so subtract 20% from the
            # size.
            #
            # This heuristic is imperfect in many ways, so in a future dirstate
            # format update it makes sense to just record the number of entries
            # on write.
            self._map = parsers.dict_new_presized(len(st) / 71)

        # Python's garbage collector triggers a GC each time a certain number
        # of container objects (the number being defined by
        # gc.get_threshold()) are allocated. parse_dirstate creates a tuple
        # for each file in the dirstate. The C version then immediately marks
        # them as not to be tracked by the collector. However, this has no
        # effect on when GCs are triggered, only on what objects the GC looks
        # into. This means that O(number of files) GCs are unavoidable.
        # Depending on when in the process's lifetime the dirstate is parsed,
        # this can get very expensive. As a workaround, disable GC while
        # parsing the dirstate.
        #
        # (we cannot decorate the function directly since it is in a C module)
        parse_dirstate = util.nogc(parsers.parse_dirstate)
        p = parse_dirstate(self._map, self.copymap, st)
        if not self._dirtyparents:
            self.setparents(*p)

        # Avoid excess attribute lookups by fast pathing certain checks
        self.__contains__ = self._map.__contains__
        self.__getitem__ = self._map.__getitem__
        self.get = self._map.get

    def write(self, st, now):
        st.write(parsers.pack_dirstate(self._map, self.copymap, self.parents(), now))
        st.close()
        self._dirtyparents = False
        self.nonnormalset, self.otherparentset = self.nonnormalentries()

    @propertycache
    def nonnormalset(self):
        nonnorm, otherparents = self.nonnormalentries()
        self.otherparentset = otherparents
        return nonnorm

    @propertycache
    def otherparentset(self):
        nonnorm, otherparents = self.nonnormalentries()
        self.nonnormalset = nonnorm
        return otherparents

    @propertycache
    def identity(self):
        self._map
        return self.identity

    @propertycache
    def dirfoldmap(self):
        f = {}
        normcase = util.normcase
        for name in self._dirs:
            f[normcase(name)] = name
        return f
