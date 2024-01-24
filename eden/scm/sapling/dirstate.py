# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# dirstate.py - working directory tracking for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# pyre-strict

from __future__ import absolute_import

import contextlib
import errno
import os
import stat
import weakref
from typing import (
    BinaryIO,
    Callable,
    Dict,
    Generator,
    Iterable,
    List,
    Optional,
    Sequence,
    Set,
    Tuple,
)

import bindings

# Using an absolute import here allows us to import localrepo even though it
# circularly imports us.
import sapling.localrepo

from . import (
    context,
    encoding,
    error,
    filesystem,
    identity,
    match as matchmod,
    pathutil,
    perftrace,
    pycompat,
    scmutil,
    transaction,
    treestate,
    ui as ui_mod,
    util,
    vfs,
)
from .i18n import _
from .node import hex, nullid
from .pycompat import encodeutf8

# pyre-fixme[5]: Global expression must be annotated.
parsers = bindings.cext.parsers

_rangemask = 0x7FFFFFFF

# pyre-fixme[5]: Global expression must be annotated.
dirstatetuple = parsers.dirstatetuple

slowstatuswarning: str = _(
    "(status will still be slow next time; try to complete or abort "
    "other source control operations and then run '@prog@ status' again)\n"
)


class repocache(scmutil.filecache):
    """filecache for files in .hg/"""

    def join(self, obj: "dirstate", fname: str) -> str:
        return obj._opener.join(fname)


def _getfsnow(vfs: "vfs.abstractvfs") -> int:
    """Get "now" timestamp on filesystem"""
    tmpfd, tmpname = vfs.mkstemp()
    try:
        return util.fstat(tmpfd).st_mtime
    finally:
        os.close(tmpfd)
        vfs.unlink(tmpname)


ParentChangeCallback = Callable[
    ["dirstate", Tuple[bytes, bytes], Tuple[bytes, bytes]], None
]


# pyre-fixme[2]: Parameter must be annotated.
def fastreadp1(repopath, is_dot_hg_path=False) -> Optional[bytes]:
    """Read dirstate p1 node without constructing repo or dirstate objects

    This is the first 20-bytes of the dirstate file. All known dirstate
    implementations (edenfs, treestate, etc.) respect this format.

    Return None if p1 cannot be read.
    """
    try:
        if not is_dot_hg_path:
            ident = identity.sniffdir(repopath)
            if not ident:
                return None
            repopath = os.path.join(repopath, ident.dotdir())

        with open(os.path.join(repopath, "dirstate"), "rb") as f:
            node = f.read(len(nullid))
            return node
    except IOError:
        return None


class dirstate:
    def __init__(
        self,
        opener: "vfs.abstractvfs",
        ui: "ui_mod.ui",
        root: str,
        validate: "Callable[[bytes], bytes]",
        repo: "sapling.localrepo.localrepository",
    ) -> None:
        """Create a new dirstate object.

        opener is an open()-like callable that can be used to open the
        dirstate file; root is the root of the directory tracked by
        the dirstate.
        """
        self._opener = opener
        self._validate = validate
        self._root = root
        self._repo: "sapling.localrepo.localrepository" = weakref.proxy(repo)
        # ntpath.join(root, '') of Python 2.7.9 does not add sep if root is
        # UNC path pointing to root share (issue4557)
        self._rootdir: str = pathutil.normasprefix(root)
        self._dirty = False
        self._lastnormaltime = 0
        self._ui = ui
        self._filecache: "Dict[str, Optional[scmutil.filecacheentry]]" = {}
        self._parentwriters = 0
        self._filename = "dirstate"
        self._pendingfilename: str = "%s.pending" % self._filename
        self._plchangecallbacks: "Dict[str, ParentChangeCallback]" = {}
        self._origpl: "Optional[Tuple[bytes, bytes]]" = None
        self._updatedfiles: "Set[str]" = set()

        self._initfs()

    # For eden_dirstate.py to override.
    def _initfs(self) -> None:
        self._opener.makedirs("treestate")

        def make_treestate(
            ui: "ui_mod.ui", opener: "vfs.abstractvfs", root: str
        ) -> "treestate.treestatemap":
            # Each time we load the treestate, make sure we have the latest
            # version.
            self._repo._rsrepo.invalidateworkingcopy()
            return treestate.treestatemap(
                ui, opener, root, self._repo._rsrepo.workingcopy().treestate()
            )

        self._mapcls = make_treestate
        self._fs = filesystem.physicalfilesystem(self._root, self)

    @contextlib.contextmanager
    def parentchange(self) -> "Generator[None, None, None]":
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

    def pendingparentchange(self) -> bool:
        """Returns true if the dirstate is in the middle of a set of changes
        that modify the dirstate parent.
        """
        return self._parentwriters > 0

    @util.propertycache
    def _map(self) -> treestate.treestatemap:
        """Return the dirstate contents (see documentation for treestatemap)."""
        self._map = self._mapcls(self._ui, self._opener, self._root)
        return self._map

    @repocache("branch")
    def _branch(self) -> str:
        return self._opener.tryreadutf8("branch").strip() or "default"

    @property
    def _pl(self) -> "Tuple[bytes, bytes]":
        return self._map.parents()

    def hasdir(self, d: str) -> bool:
        return self._map.hastrackeddir(d)

    @util.propertycache
    def _ignore(self) -> "matchmod.gitignorematcher":
        # gitignore
        globalignores = self._globalignorefiles()
        return matchmod.gitignorematcher(self._root, "", gitignorepaths=globalignores)

    @util.propertycache
    def _slash(self) -> bool:
        return (
            self._ui.plain() or self._ui.configbool("ui", "slash")
        ) and pycompat.ossep != "/"

    @util.propertycache
    def _checklink(self) -> bool:
        if pycompat.iswindows and "windowssymlinks" not in self._repo.requirements:
            return False
        return util.checklink(self._root)

    @util.propertycache
    def _checkexec(self) -> bool:
        return util.checkexec(self._root)

    @util.propertycache
    def _checkcase(self) -> bool:
        return not util.fscasesensitive(self._repo.path)

    def _join(self, f: str) -> str:
        # much faster than os.path.join()
        # it's safe because f is always a relative path
        return self._rootdir + f

    def flagfunc(
        self, buildfallback: "Callable[[], Callable[[str], str]]"
    ) -> "Callable[[str], str]":
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

        fallback: "Callable[[str], str]" = buildfallback()
        if self._checklink:

            def f(x):
                if os.path.islink(self._join(x)):
                    return "l"
                if "x" in fallback(x):
                    return "x"
                return ""

            return f
        if self._checkexec:

            def f(x: str) -> str:
                if "l" in fallback(x):
                    return "l"
                if util.isexec(self._join(x)):
                    return "x"
                return ""

            return f
        else:
            return fallback

    @util.propertycache
    def _cwd(self) -> str:
        # internal config: ui.forcecwd
        forcecwd = self._ui.config("ui", "forcecwd")
        if forcecwd:
            return forcecwd
        return pycompat.getcwd()

    def getcwd(self) -> str:
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

    def pathto(self, f: str, cwd: "Optional[str]" = None) -> str:
        if cwd is None:
            cwd = self.getcwd()
        path = util.pathto(self._root, cwd, f)
        if self._slash:
            return util.pconvert(path)
        return path

    def __getitem__(self, key: str) -> str:
        """Return the current state of key (a filename) in the dirstate.

        States are:
          n  normal
          m  needs merging
          r  marked for removal
          a  marked for addition
          ?  not tracked
        """
        return self._map.get(key, ("?", 0, 0, 0))[0]

    def __contains__(self, key: str) -> bool:
        return key in self._map

    def __iter__(self) -> "Iterable[str]":
        return iter(sorted(self._map))

    # pyre-fixme[11]: Annotation `dirstatetuple` is not defined as a type.
    def items(self) -> "Iterable[Tuple[str, dirstatetuple]]":
        return pycompat.iteritems(self._map)

    iteritems: "Callable[[dirstate], Iterable[Tuple[str, dirstatetuple]]]" = items

    def parents(self) -> "List[bytes]":
        # (This always returns a list of length 2.  Perhaps we should change it to
        # return a tuple instead.)
        return [self._validate(p) for p in self._pl]

    def p1(self) -> bytes:
        return self._validate(self._pl[0])

    def p2(self) -> bytes:
        return self._validate(self._pl[1])

    def branch(self) -> str:
        return encoding.tolocal(self._branch)

    def setparents(self, p1: bytes, p2: bytes = nullid) -> "Dict[str, str]":
        """Set dirstate parents to p1 and p2.

        When moving from two parents to one, 'm' merged entries a
        adjusted to normal and previous copy records discarded and
        returned by the call.

        See localrepo.setparents()
        """
        if self._parentwriters == 0:
            raise ValueError(
                "cannot set dirstate parent without " "calling dirstate.parentchange"
            )

        self._dirty = True
        oldp2 = self._pl[1]
        if self._origpl is None:
            self._origpl = self._pl
        self._map.setparents(p1, p2)
        copies: "Dict[str, str]" = {}
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

    def setbranch(self, branch: str) -> None:
        assert isinstance(branch, str)
        self._branch = encoding.fromlocal(branch)
        f = self._opener("branch", "w", atomictemp=True, checkambig=True)
        try:
            f.write(encodeutf8(self._branch + "\n"))
            f.close()

            # make sure filecache has the correct stat info for _branch after
            # replacing the underlying file
            ce = self._filecache["_branch"]
            if ce:
                ce.refresh()
        except:  # re-raises
            # pyre-fixme[16]: `BinaryIO` has no attribute `discard`.
            f.discard()
            raise

    def invalidate(self) -> None:
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

    def copy(self, source: str, dest: str) -> None:
        """Mark dest as a copy of source. Unmark dest if source is None."""
        if source == dest:
            return
        self._dirty = True
        self._map.copy(source, dest)

    def copied(self, file: str) -> "Optional[str]":
        return self._map.copysource(file)

    def copies(self) -> "Dict[str, str]":
        return self._map.copymap

    def needcheck(self, file: str) -> bool:
        """Mark file as need-check"""
        changed = self._map.needcheck(file)
        self._dirty |= changed
        return changed

    def clearneedcheck(self, file: str) -> None:
        changed = self._map.clearneedcheck(file)
        self._dirty |= changed

    def setclock(self, clock: str) -> None:
        """Set fsmonitor clock"""
        return self.setmeta("clock", clock)

    def getclock(self) -> "Optional[str]":
        """Get fsmonitor clock"""
        return self.getmeta("clock")

    def setmeta(self, name: str, value: "Optional[str]") -> None:
        """Set metadata"""
        value = value or None
        if value != self.getmeta(name):
            self._map.updatemetadata({name: value})
            self._dirty = True

    def getmeta(self, name: str) -> "Optional[str]":
        """Get metadata"""
        # Normalize "" to "None"
        return self._map.getmetadata().get(name) or None

    def _addpath(self, f: str, state: str, mode: int, size: int, mtime: int) -> None:
        oldstate = self[f]
        if state == "a" or oldstate == "r":
            scmutil.checkfilename(f)
            if self._map.hastrackeddir(f):
                raise error.Abort(_("directory %r already in dirstate") % f)
            if os.path.isabs(f):
                raise error.Abort(
                    _("cannot add non-root-relative path to dirstate: %s") % f
                )
            relativedirs = [".", ".."]
            if any(s for s in f.split("/") if s in relativedirs):
                raise error.Abort(_("cannot add path with relative parents: %s") % f)

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

    def normal(self, f: str) -> None:
        """Mark a file normal and clean."""
        s = util.lstat(self._join(f))
        mtime = s.st_mtime
        self._addpath(f, "n", s.st_mode, s.st_size & _rangemask, mtime & _rangemask)
        if mtime > self._lastnormaltime:
            # Remember the most recent modification timeslot for status(),
            # to make sure we won't miss future size-preserving file content
            # modifications that happen within the same timeslot.
            self._lastnormaltime = mtime

    def normallookup(self, f: str) -> None:
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

    def otherparent(self, f: str) -> None:
        """Mark as coming from the other parent, always dirty."""
        if self._pl[1] == nullid:
            raise error.Abort(
                _("setting %r to other parent " "only allowed in merges") % f
            )

        entry = self._map.get(f)
        if entry is not None and entry[0] == "n" and entry[2] != -2:
            # merge-like
            self._addpath(f, "m", 0, -2, -1)
        else:
            # add-like
            self._addpath(f, "n", 0, -2, -1)

    def add(self, f: str) -> None:
        """Mark a file added."""
        self._addpath(f, "a", 0, -1, -1)

    def remove(self, f: str) -> None:
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
        self._updatedfiles.add(f)
        self._map.removefile(f, oldstate, size)

    def merge(self, f: str) -> None:
        """Mark a file merged."""
        if self._pl[1] == nullid:
            return self.normallookup(f)
        return self.otherparent(f)

    def untrack(self, f: str) -> None:
        """Stops tracking a file in the dirstate. This is useful during
        operations that want to stop tracking a file, but still have it show up
        as untracked (like hg forget)."""
        oldstate = self[f]
        if self._map.untrackfile(f, oldstate):
            self._dirty = True

    def delete(self, f: str) -> None:
        """Removes a file from the dirstate entirely. This is useful during
        operations like update, to remove files from the dirstate that are known
        to be deleted."""
        oldstate = self[f]
        if self._map.deletefile(f, oldstate):
            self._dirty = True

    def _discoverpath(
        self,
        path: str,
        normed: str,
        ignoremissing: bool,
        exists: "Optional[bool]",
        storemap: "Dict[str, str]",
    ) -> str:
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

    def _normalize(
        self,
        path: str,
        isknown: bool,
        ignoremissing: bool = False,
        exists: "Optional[bool]" = None,
    ) -> str:
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

    def normalize(
        self, path: str, isknown: bool = False, ignoremissing: bool = False
    ) -> str:
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

    def clear(self) -> None:
        self._map.clear()
        self._lastnormaltime = 0
        self._updatedfiles.clear()
        self._dirty = True

    def rebuild(
        self,
        parent: bytes,
        allfiles: "Sequence[str]",
        changedfiles: "Optional[Sequence[str]]" = None,
        exact: bool = False,
    ) -> None:
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

    def identity(self) -> object:
        """Return identity of dirstate itself to detect changing in storage

        If identity of previous dirstate is equal to this, writing
        changes based on the former dirstate out can keep consistency.
        """
        return self._map.identity

    def write(self, tr: "Optional[transaction.transaction]") -> None:
        if not self._dirty:
            return

        filename = self._filename
        if tr:
            self._markforwrite()
            return

        st = self._opener(filename, "w", atomictemp=True, checkambig=True)
        self._writedirstate(st)

    def _markforwrite(self) -> None:
        tr = self._repo.currenttransaction()
        if not tr:
            raise error.ProgrammingError("no transaction during dirstate write")

        self._dirty = True

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

    @util.propertycache
    def checkoutidentifier(self) -> str:
        try:
            return self._opener.readutf8("checkoutidentifier")
        except IOError as e:
            if e.errno != errno.ENOENT:
                raise
        return ""

    def addparentchangecallback(
        self, category: str, callback: "ParentChangeCallback"
    ) -> None:
        """add a callback to be called when the wd parents are changed

        Callback will be called with the following arguments:
            dirstate, (oldp1, oldp2), (newp1, newp2)

        Category is a unique identifier to allow overwriting an old callback
        with a newer callback.
        """
        self._plchangecallbacks[category] = callback

    def _writedirstate(self, st: "BinaryIO") -> None:
        # notify callbacks about parents change
        origpl = self._origpl
        if origpl is not None and origpl != self._pl:
            for c, callback in sorted(pycompat.iteritems(self._plchangecallbacks)):
                callback(self, origpl, self._pl)
            # if the first parent has changed then consider this a new checkout
            if origpl[0] != self._pl[0]:
                with self._opener("checkoutidentifier", "w", atomictemp=True) as f:
                    f.write(util.makerandomidentifier().encode("utf-8"))
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
            for f, e in pycompat.iteritems(self._map):
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

    def _dirignore(self, f: str) -> bool:
        if f == "":
            return False
        visitdir = self._ignore.visitdir
        if visitdir(f) == "all":
            return True
        return False

    def _ignorefiles(self) -> "List[str]":
        files = []
        files += self._globalignorefiles()
        return files

    def _globalignorefiles(self) -> "List[str]":
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
    def status(
        self, match: matchmod.basematcher, ignored: bool, clean: bool, unknown: bool
    ) -> scmutil.status:
        """Determine the status of the working copy relative to the
        dirstate and return a scmutil.status.
        """
        if self._lastnormaltime > 0:
            # This handles situations where, for example:
            #   1. File "foo" is NEED_CHECK in treestate, but contents are clean.
            #   2. "sl import" performs an update at time1 that marks "foo" as clean in-memory,
            #      but doesn't write dirstate out yet.
            #   3. "sl import" changes "foo" contents on disk with mtime time1.
            #   4. "status" needs to list "foo" as modified even though mtime matches treestate.
            #
            # _lastnormaltime is set during step 2. We used to pass _lastnormaltime
            # directly to workingcopy().status(), but now we re-use the invalidatemtime()
            # mechanism to mark "foo" as NEED_CHECK.
            self._map._tree.invalidatemtime(self._lastnormaltime)

        status = self._repo._rsrepo.workingcopy().status(
            match, bool(ignored), self._ui._rcfg
        )

        if not unknown:
            status.unknown.clear()

        for invalid in status.invalid_path:
            self._ui.warn(_("skipping invalid path %r\n") % invalid)

        self._add_clean_and_trigger_bad_matches(
            match,
            status,
            self._repo[None].p1(),
            clean,
            pathutil.pathauditor(self._root, cached=True),
        )

        return status

    def _add_clean_and_trigger_bad_matches(
        self,
        match: matchmod.basematcher,
        status: scmutil.status,
        pctx: context.changectx,
        listclean: bool,
        auditor: pathutil.pathauditor,
    ) -> None:
        seenset = set()
        for files in status:
            seenset.update(files)
            seenset.update(util.dirs(files))

        if listclean:
            clean = status.clean
            for fn in pctx.manifest().matches(match):
                assert isinstance(fn, str)
                if fn not in seenset:
                    clean.append(fn)
            seenset.update(clean)

        for path in sorted(match.files()):
            # path can be "". Rust doesn't do this, so this "if" can go away later.
            if path:
                auditor(path)

            try:
                st = os.lstat(os.path.join(self._root, path))
            except OSError as ex:
                if path not in seenset:
                    # This handles does-not-exist, permission error, etc.
                    match.bad(path, encoding.strtolocal(ex.strerror))
                continue

            typ = stat.S_IFMT(st.st_mode)
            if not typ & (stat.S_IFDIR | stat.S_IFREG | stat.S_IFLNK):
                # This handles invalid types like named pipe.
                match.bad(path, filesystem.badtype(typ))

    def matches(self, match: matchmod.basematcher) -> "Iterable[str]":
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

        return dmap.matches(match)

    def _actualfilename(self, tr: "Optional[transaction.transaction]") -> str:
        if tr:
            return self._pendingfilename
        else:
            return self._filename

    def savebackup(
        self, tr: "Optional[transaction.transaction]", backupname: str
    ) -> None:
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

    def restorebackup(
        self, tr: "Optional[transaction.transaction]", backupname: str
    ) -> None:
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

    def clearbackup(
        self, tr: "Optional[transaction.transaction]", backupname: str
    ) -> None:
        """Clear backup file"""
        self._opener.unlink(backupname)

    def loginfo(self, ui: "ui_mod.ui", prefix: str) -> None:
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
