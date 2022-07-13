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
import tempfile
import weakref
from typing import (
    BinaryIO,
    Callable,
    cast,
    Dict,
    Generator,
    Iterable,
    List,
    Optional,
    Sequence,
    Set,
    Tuple,
    Type,
    Union,
)

import bindings

# Using an absolute import here allows us to import localrepo even though it
# circularly imports us.
import edenscm.mercurial.localrepo
from edenscmnative import parsers

from . import (
    context,
    encoding,
    error,
    filesystem,
    match as matchmod,
    pathutil,
    perftrace,
    pycompat,
    scmutil,
    transaction,
    treedirstate,
    treestate,
    txnutil,
    ui as ui_mod,
    util,
    vfs,
)
from .i18n import _
from .node import hex, nullid
from .pycompat import encodeutf8


_rangemask = 0x7FFFFFFF

dirstatetuple = parsers.dirstatetuple

slowstatuswarning: str = _(
    "(status will still be slow next time; try to complete or abort "
    "other source control operations and then run 'hg status' again)\n"
)


class repocache(scmutil.filecache):
    """filecache for files in .hg/"""

    def join(self, obj: "dirstate", fname: str) -> str:
        return obj._opener.join(fname)


class rootcache(scmutil.filecache):
    """filecache for files in the repository root"""

    def join(self, obj: "dirstate", fname: str) -> str:
        return obj._join(fname)


def _getfsnow(vfs: "vfs.abstractvfs") -> int:
    """Get "now" timestamp on filesystem"""
    tmpfd, tmpname = vfs.mkstemp()
    try:
        return util.fstat(tmpfd).st_mtime
    finally:
        os.close(tmpfd)
        vfs.unlink(tmpname)


DirstateMapClassType = Union[
    Type["dirstatemap"],
    Type[treestate.treestatemap],
    Type[treedirstate.treedirstatemap],
]
DirstateMapType = Union[
    "dirstatemap", treestate.treestatemap, treedirstate.treedirstatemap
]
ParentChangeCallback = Callable[
    ["dirstate", Tuple[bytes, bytes], Tuple[bytes, bytes]], None
]


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def fastreadp1(repopath):
    """Read dirstate p1 node without constructing repo or dirstate objects

    This is the first 20-bytes of the dirstate file. All known dirstate
    implementations (edenfs, treestate, etc.) respect this format.

    Return None if p1 cannot be read.
    """
    try:
        with open(os.path.join(repopath, ".hg", "dirstate"), "rb") as f:
            node = f.read(len(nullid))
            return node
    except IOError:
        return None


class dirstate(object):
    def __init__(
        self,
        opener: "vfs.abstractvfs",
        ui: "ui_mod.ui",
        root: str,
        validate: "Callable[[bytes], bytes]",
        repo: "edenscm.mercurial.localrepo.localrepository",
        istreestate: bool = False,
        istreedirstate: bool = False,
    ) -> None:
        """Create a new dirstate object.

        opener is an open()-like callable that can be used to open the
        dirstate file; root is the root of the directory tracked by
        the dirstate.
        """
        self._opener = opener
        self._validate = validate
        self._root = root
        self._repo: "edenscm.mercurial.localrepo.localrepository" = weakref.proxy(repo)
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
        # TODO(quark): after migrating to treestate, remove legacy code.
        self._istreestate = istreestate
        self._istreedirstate = istreedirstate
        if istreestate:
            opener.makedirs("treestate")
            self._mapcls: "DirstateMapClassType" = treestate.treestatemap
        elif istreedirstate:
            ui.deprecate("treedirstate", "treedirstate is replaced by treestate")
            self._mapcls: "DirstateMapClassType" = treedirstate.treedirstatemap
        else:
            if "eden" not in repo.requirements:
                ui.deprecate("dirstatemap", "dirstatemap is replaced by treestate")
            self._mapcls: "DirstateMapClassType" = dirstatemap
        self._fs = filesystem.physicalfilesystem(root, self)

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

    def beginparentchange(self) -> None:
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

    def endparentchange(self) -> None:
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

    def pendingparentchange(self) -> bool:
        """Returns true if the dirstate is in the middle of a set of changes
        that modify the dirstate parent.
        """
        return self._parentwriters > 0

    @util.propertycache
    def _map(self) -> "DirstateMapType":
        """Return the dirstate contents (see documentation for dirstatemap)."""
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

    @rootcache(".hgignore")
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
        return util.checklink(self._root)

    @util.propertycache
    def _checkexec(self) -> bool:
        return util.checkexec(self._root)

    @util.propertycache
    def _checkcase(self) -> bool:
        return not util.fscasesensitive(self._join(".hg"))

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
        # pyre-fixme[6]: expected Iterable (maybe PEP 544 will fix this?)
        return iter(sorted(self._map))

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
                "cannot set dirstate parent without "
                "calling dirstate.beginparentchange"
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
        if self._istreestate:
            tsmap = cast(treestate.treestatemap, self._map)
            tsmap.copy(source, dest)
            # treestatemap.copymap needs to be changed via the "copy" method.
            # _updatedfiles is not used by treestatemap as it's tracked
            # internally.
            return
        dmap = cast(Union[dirstatemap, treedirstate.treedirstatemap], self._map)
        if source is not None:
            dmap.copymap[dest] = source
            self._updatedfiles.add(source)
            self._updatedfiles.add(dest)
        elif dmap.copymap.pop(dest, None):
            self._updatedfiles.add(dest)

    def copied(self, file: str) -> "Optional[str]":
        if self._istreestate:
            tsmap = cast(treestate.treestatemap, self._map)
            return tsmap.copysource(file)
        else:
            dmap = cast(Union[dirstatemap, treedirstate.treedirstatemap], self._map)
            return dmap.copymap.get(file, None)

    def copies(self) -> "Dict[str, str]":
        return self._map.copymap

    def needcheck(self, file: str) -> bool:
        """Mark file as need-check"""
        if not self._istreestate:
            raise error.ProgrammingError("needcheck is only supported by treestate")
        tsmap = cast(treestate.treestatemap, self._map)
        changed = tsmap.needcheck(file)
        self._dirty |= changed
        return changed

    def clearneedcheck(self, file: str) -> None:
        if not self._istreestate:
            raise error.ProgrammingError("needcheck is only supported by treestate")
        tsmap = cast(treestate.treestatemap, self._map)
        changed = tsmap.clearneedcheck(file)
        self._dirty |= changed

    def setclock(self, clock: str) -> None:
        """Set fsmonitor clock"""
        return self.setmeta("clock", clock)

    def getclock(self) -> "Optional[str]":
        """Get fsmonitor clock"""
        return self.getmeta("clock")

    def setmeta(self, name: str, value: "Optional[str]") -> None:
        """Set metadata"""
        if not self._istreestate:
            raise error.ProgrammingError("setmeta is only supported by treestate")
        value = value or None
        if value != self.getmeta(name):
            tsmap = cast(treestate.treestatemap, self._map)
            tsmap.updatemetadata({name: value})
            self._dirty = True

    def getmeta(self, name: str) -> "Optional[str]":
        """Get metadata"""
        if not self._istreestate:
            raise error.ProgrammingError("getmeta is only supported by treestate")
        tsmap = cast(treestate.treestatemap, self._map)
        # Normalize "" to "None"
        return tsmap.getmetadata().get(name) or None

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
        if not self._istreestate:
            self._map.copymap.pop(f, None)
            if f in self._map.nonnormalset:
                self._map.nonnormalset.remove(f)
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
        if not self._istreestate:
            self._map.copymap.pop(f, None)

    def otherparent(self, f: str) -> None:
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

    def add(self, f: str) -> None:
        """Mark a file added."""
        self._addpath(f, "a", 0, -1, -1)
        if not self._istreestate:
            self._map.copymap.pop(f, None)

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
                    if not self._istreestate:
                        self._map.otherparentset.add(f)
        self._updatedfiles.add(f)
        self._map.removefile(f, oldstate, size)
        if not self._istreestate:
            if size == 0:
                self._map.copymap.pop(f, None)

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
            if not self._istreestate:
                self._updatedfiles.add(f)
                self._map.copymap.pop(f, None)

    def delete(self, f: str) -> None:
        """Removes a file from the dirstate entirely. This is useful during
        operations like update, to remove files from the dirstate that are known
        to be deleted."""
        oldstate = self[f]
        if self._map.deletefile(f, oldstate):
            self._dirty = True
            if not self._istreestate:
                self._updatedfiles.add(f)
                self._map.copymap.pop(f, None)

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

    def _normalizefile(
        self,
        path: str,
        isknown: bool,
        ignoremissing: bool = False,
        exists: "Optional[bool]" = None,
    ) -> str:
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

    class FallbackToPythonStatus(Exception):
        pass

    def _ruststatus(
        self, match: "Callable[[str], bool]", ignored: bool, clean: bool, unknown: bool
    ) -> "scmutil.status":
        if util.safehasattr(self._fs, "_fsmonitorstate"):
            filesystem = "watchman"
        elif "eden" in self._repo.requirements:
            filesystem = "eden"
        else:
            filesystem = "normal"

        # TODO: Fix deadlock in normal filesystem crawler
        if ignored or clean or filesystem == "normal":
            raise self.FallbackToPythonStatus

        if filesystem == "eden":
            # EdenFS repos still use an old dirstate to track working copy
            # changes. We need a TreeState for Rust status, so if the map
            # doesn't have a tree, we create a temporary read-only one.
            # Note: this TreeState won't track clean files, only added/removed/etc.
            # TODO: get rid of this when EdenFS migrates to TreeState.
            tempdir = tempfile.TemporaryDirectory()
            tempvfs = vfs.vfs(tempdir.name)
            tempvfs.makedir("treestate")
            tempmap = treestate.treestatemap(
                self._ui, tempvfs, tempdir.name, importdirstate=self
            )
            tree = tempmap._tree
        else:
            # pyre-fixme[16]: Item `dirstatemap` of `Union[dirstatemap,
            #  treedirstatemap, treestatemap]` has no attribute `_tree`.
            tree = self._map._tree

        # TODO: Handle the case that a file is ignored but is still tracked
        # in p1.
        match = matchmod.differencematcher(match, self._ignore)

        return bindings.workingcopy.status.status(
            self._root,
            self._repo[self.p1()].manifest(),
            self._repo.fileslog.filescmstore,
            tree,
            self._lastnormaltime,
            match,
            unknown,
            filesystem,
        )

    @perftrace.tracefunc("Status")
    def status(
        self, match: "Callable[[str], bool]", ignored: bool, clean: bool, unknown: bool
    ) -> "scmutil.status":
        """Determine the status of the working copy relative to the
        dirstate and return a scmutil.status.
        """
        if self._ui.configbool("workingcopy", "ruststatus"):
            try:
                return self._ruststatus(match, ignored, clean, unknown)
            except self.FallbackToPythonStatus:
                pass

        wctx = self._repo[None]
        # Prime the wctx._parents cache so the parent doesn't change out from
        # under us if a checkout happens in another process.
        pctx = wctx.p1()

        listignored, listclean, listunknown = ignored, clean, unknown
        modified: "List[str]" = []
        added: "List[str]" = []
        unknownpaths: "List[str]" = []
        ignoredpaths: "List[str]" = []
        removed: "List[str]" = []
        deleted: "List[str]" = []
        cleanpaths: "List[str]" = []

        dmap = self._map
        dmap.preload()
        dget = dmap.__getitem__
        madd = modified.append
        aadd = added.append
        uadd = unknownpaths.append
        iadd = ignoredpaths.append
        radd = removed.append
        dadd = deleted.append
        cadd = cleanpaths.append
        ignore = self._ignore
        copymap = self._map.copymap

        # We have seen some rare issues that a few "M" or "R" files show up
        # while the files are expected to be clean. Log the reason of first few
        # "M" files.
        mtolog = self._ui.configint("experimental", "samplestatus") or 0

        oldid = self.identity()

        # Step 1: Get the files that are different from the clean checkedout p1 tree.
        pendingchanges = self._fs.pendingchanges(match, listignored=listignored)

        for fn, exists in pendingchanges:
            assert isinstance(fn, str)
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
                madd(fn)
            else:
                # All other states will be handled by the logic below, and we
                # don't care that it's a pending change.
                pass

        # Fetch the nonnormalset after iterating over pendingchanges, since the
        # iteration may change the nonnormalset as lookup states are resolved.
        if util.safehasattr(dmap, "nonnormalsetfiltered"):
            # treestate has a fast path to filter out ignored directories.
            ignorevisitdir: "Callable[[str], Union[str, bool]]" = ignore.visitdir

            def dirfilter(path: str) -> bool:
                result = ignorevisitdir(path.rstrip("/"))
                return result == "all"

            tsmap = cast(treestate.treestatemap, dmap)
            nonnormalset = tsmap.nonnormalsetfiltered(dirfilter)
        else:
            nonnormalset = dmap.nonnormalset

        otherparentset = dmap.otherparentset

        # The seen set is used to prevent steps 2 and 3 from processing things
        # we saw in step 1.
        seenset = set(deleted + modified)

        # audit_path is used to verify that nonnormal files still exist and are
        # not behind symlinks.
        auditpath: "pathutil.pathauditor" = pathutil.pathauditor(
            self._root, cached=True
        )

        def fileexists(fn: str) -> bool:
            # So let's double check for the existence of that file.
            st = list(util.statfiles([self._join(fn)]))[0]

            # auditpath checks to see if the file is under a symlink directory.
            # If it is, we treat it the same as if it didn't exist.
            return st is not None and auditpath.check(fn)

        # Step 2: Handle status results that are not simply pending filesystem
        # changes on top of the pristine tree.
        for fn in otherparentset:
            assert isinstance(fn, str)
            if not match(fn) or fn in seenset:
                continue
            t = dget(fn)
            state = t[0]
            # We only need to handle 'n' here, since all other states will be
            # covered by the nonnormal loop below.
            if state in "n":
                # pendingchanges() above only checks for changes against p1.
                # For things from p2, we need to manually check for
                # existence. We don't have to check if they're modified,
                # since them coming from p2 indicates they are considered
                # modified.
                if fileexists(fn):
                    if mtolog > 0:
                        mtolog -= 1
                        self._ui.log("status", "M %s: exists in p2" % fn)
                    madd(fn)
                else:
                    dadd(fn)
                seenset.add(fn)

        for fn in nonnormalset:
            assert isinstance(fn, str)
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
                if fileexists(fn):
                    aadd(fn)
                else:
                    # If an added file is deleted, report it as missing
                    dadd(fn)
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
            assert isinstance(fn, str)
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
            modified, added, removed, deleted, unknownpaths, ignoredpaths, cleanpaths
        )

        # Step 3: If clean files were requested, add those to the results
        seenset = set()
        for files in status:
            seenset.update(files)
            seenset.update(util.dirs(files))

        if listclean:
            for fn in pctx.manifest().matches(match):
                assert isinstance(fn, str)
                if fn not in seenset:
                    cadd(fn)
            seenset.update(cleanpaths)

        # Step 4: Report any explicitly requested files that don't exist
        # pyre-fixme[16]: Anonymous callable has no attribute `files`.
        for path in sorted(match.files()):
            try:
                if path in seenset:
                    continue
                os.lstat(os.path.join(self._root, path))
            except OSError as ex:
                # pyre-fixme[16]: Anonymous callable has no attribute `bad`.
                match.bad(path, encoding.strtolocal(ex.strerror))

        # TODO: fire this inside filesystem. fixup is a list of files that
        # checklookup says are clean
        if not getattr(self._repo, "_insidepoststatusfixup", False):
            self._poststatusfixup(status, wctx, oldid)

        perftrace.tracevalue("A/M/R Files", len(modified) + len(added) + len(removed))
        if len(unknownpaths) > 0:
            perftrace.tracevalue("Unknown Files", len(unknownpaths))
        if len(ignoredpaths) > 0:
            perftrace.tracevalue("Ignored Files", len(ignoredpaths))
        return status

    def _poststatusfixup(
        self, status: "scmutil.status", wctx: "context.workingctx", oldid: object
    ) -> None:
        """update dirstate for files that are actually clean"""
        poststatusbefore = self._repo.postdsstatus(afterdirstatewrite=False)
        poststatusafter = self._repo.postdsstatus(afterdirstatewrite=True)
        ui = self._repo.ui
        if poststatusbefore or poststatusafter or self._dirty:
            # prevent infinite loop because fsmonitor postfixup might call
            # wctx.status()
            # pyre-fixme[16]: localrepo has no attribute _insidepoststatusfixup
            self._repo._insidepoststatusfixup = True
            try:
                # Updating the dirstate is optional so we don't wait on the
                # lock.
                # wlock can invalidate the dirstate, so cache normal _after_
                # taking the lock. This is a bit weird because we're inside the
                # dirstate that is no longer valid.

                # If watchman reports fresh instance, still take the lock,
                # since not updating watchman state leads to very painful
                # performance.
                freshinstance = False
                nonnormalcount = 0
                try:
                    # pyre-fixme[16]: physicalfilesystem has no attr _fsmonitorstate
                    freshinstance = self._fs._fsmonitorstate._lastisfresh
                    nonnormalcount = self._fs._fsmonitorstate._lastnonnormalcount
                except Exception:
                    pass
                waitforlock = False
                nonnormalthreshold = self._repo.ui.configint(
                    "fsmonitor", "dirstate-nonnormal-file-threshold"
                )
                if (
                    nonnormalthreshold is not None
                    and nonnormalcount >= nonnormalthreshold
                ):
                    ui.debug(
                        "poststatusfixup decides to wait for wlock since nonnormal file count %s >= %s\n"
                        % (nonnormalcount, nonnormalthreshold)
                    )
                    waitforlock = True
                if freshinstance:
                    waitforlock = True
                    ui.debug(
                        "poststatusfixup decides to wait for wlock since watchman reported fresh instance\n"
                    )

                with self._repo.disableeventreporting(), self._repo.wlock(waitforlock):
                    identity = self._repo.dirstate.identity()
                    if identity == oldid:
                        if poststatusbefore:
                            for ps in poststatusbefore:
                                ps(wctx, status)

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
                    elif not util.istest():
                        # Too noisy in tests.
                        ui.debug(
                            "poststatusfixup did not write dirstate because identity changed %s != %s\n"
                            % (oldid, identity)
                        )

            except error.LockError as ex:
                # pyre-fixme[61]: `waitforlock` may not be initialized here.
                if waitforlock:
                    ui.write_err(
                        _(
                            "warning: failed to update watchman state because wlock cannot be obtained (%s)\n"
                        )
                        % (ex,)
                    )
                    ui.write_err(slowstatuswarning)
                else:
                    ui.debug(
                        "poststatusfixup did not write dirstate because wlock cannot be obtained (%s)\n"
                        % (ex,)
                    )

            finally:
                # Even if the wlock couldn't be grabbed, clear out the list.
                self._repo.clearpostdsstatus()
                self._repo._insidepoststatusfixup = False

    def matches(self, match: "matchmod.basematcher") -> "Iterable[str]":
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
                tsmap = cast(treestate.treestatemap, dmap)
                result = set()
                fastpathvalid = True
                for prefix in files:
                    if prefix in tsmap:
                        # prefix is a file
                        result.add(prefix)
                    elif tsmap.hastrackeddir(prefix + "/"):
                        # prefix is a directory
                        result.update(tsmap.keys(prefix=prefix + "/"))
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
        elif self._istreestate:
            # Treestate native path. Avoid visiting directories.
            # pyre-fixme[16]: Item `dirstatemap` of `Union[dirstatemap,
            #  treedirstatemap, treestatemap]` has no attribute `matches`.
            return dmap.matches(match)
        # Slow path: scan all files in dirstate.
        return [f for f in dmap if match(f)]

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

    def __init__(self, ui: "ui_mod.ui", opener: "vfs.abstractvfs", root: str) -> None:
        self._ui = ui
        self._opener = opener
        self._root = root
        self._filename = "dirstate"

        self._parents: "Optional[Tuple[bytes, bytes]]" = None
        self._dirtyparents = False

        # for consistent view between _pl() and _read() invocations
        self._pendingmode: "Optional[bool]" = None

    @util.propertycache
    def _map(self) -> "Dict[str, dirstatetuple]":
        self._map = {}
        self.read()
        return self._map

    @util.propertycache
    def copymap(self) -> "Dict[str, str]":
        self.copymap = {}
        self._map
        return self.copymap

    def clear(self) -> None:
        self._map.clear()
        self.copymap.clear()
        self.setparents(nullid, nullid)
        util.clearcachedproperty(self, "_dirs")
        util.clearcachedproperty(self, "_alldirs")
        util.clearcachedproperty(self, "filefoldmap")
        util.clearcachedproperty(self, "dirfoldmap")
        util.clearcachedproperty(self, "nonnormalset")
        util.clearcachedproperty(self, "otherparentset")

    def iteritems(self) -> "Iterable[Tuple[str, dirstatetuple]]":
        return pycompat.iteritems(self._map)

    def items(self) -> "Iterable[Tuple[str, dirstatetuple]]":
        return pycompat.iteritems(self._map)

    def __len__(self) -> int:
        return len(self._map)

    def __iter__(self) -> "Iterable[str]":
        return iter(self._map)

    def get(
        self, key: str, default: "Optional[dirstatetuple]" = None
    ) -> "Optional[dirstatetuple]":
        return self._map.get(key, default)

    def __contains__(self, key: str) -> bool:
        return key in self._map

    def __getitem__(self, key: str) -> "dirstatetuple":
        return self._map[key]

    def keys(self) -> "Iterable[str]":
        return self._map.keys()

    def preload(self) -> None:
        """Loads the underlying data, if it's not already loaded"""
        self._map

    def addfile(
        self, f: str, oldstate: str, state: str, mode: int, size: int, mtime: int
    ) -> None:
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

    def removefile(self, f: str, oldstate: str, size: int) -> None:
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

    def deletefile(self, f: str, oldstat: str) -> None:
        """
        Removes a file from the dirstate entirely, implying it doesn't even
        exist on disk anymore and may not be untracked.
        """
        # In the default dirstate implementation, deletefile is the same as
        # untrackfile.
        self.untrackfile(f, oldstat)

    def untrackfile(self, f: str, oldstate: str) -> bool:
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

    def clearambiguoustimes(self, files: "Iterable[str]", now: int) -> None:
        for f in files:
            e = self.get(f)
            if e is not None and e[0] == "n" and e[3] == now:
                self._insert_tuple(f, e[0], e[1], e[2], -1)
                self.nonnormalset.add(f)

    def _insert_tuple(
        self, f: str, state: str, mode: int, size: int, mtime: int
    ) -> None:
        self._map[f] = dirstatetuple(state, mode, size, mtime)

    def nonnormalentries(self) -> "Tuple[Set[str], Set[str]]":
        """Compute the nonnormal dirstate entries from the dmap"""
        try:
            return parsers.nonnormalotherparententries(self._map)
        except AttributeError:
            nonnorm = set()
            otherparent = set()
            for fname, e in pycompat.iteritems(self._map):
                if e[0] != "n" or e[3] == -1:
                    nonnorm.add(fname)
                if e[0] == "n" and e[2] == -2:
                    otherparent.add(fname)
            return nonnorm, otherparent

    @util.propertycache
    def filefoldmap(self) -> "Dict[str, str]":
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
        for name, s in pycompat.iteritems(self._map):
            if s[0] != "r":
                f[normcase(name)] = name
        f["."] = "."  # prevents useless util.fspath() invocation
        return f

    def hastrackeddir(self, d: str) -> bool:
        """
        Returns True if the dirstate contains a tracked (not removed) file
        in this directory.
        """
        return d in self._dirs

    def hasdir(self, d: str) -> bool:
        """
        Returns True if the dirstate contains a file (tracked or removed)
        in this directory.
        """
        return d in self._alldirs

    @util.propertycache
    # pyre-fixme[3]: Return type must be annotated.
    def _dirs(self):
        """
        Build a set of directories present in the dirstate.

        Some dirstate implementation (eden for instance), don't support this,
        and it's thus a good idea to test if "_dirs" is in self.__dict__.
        """
        return util.dirs((p for (p, s) in pycompat.iteritems(self._map) if s[0] != "r"))

    @util.propertycache
    # pyre-fixme[3]: Return type must be annotated.
    def _alldirs(self):
        return util.dirs(self._map)

    def _opendirstatefile(self) -> "BinaryIO":
        fp, mode = txnutil.trypending(self._root, self._opener, self._filename)
        if self._pendingmode is not None and self._pendingmode != mode:
            fp.close()
            raise error.Abort(_("working directory state may be changed parallelly"))
        self._pendingmode = mode
        return fp

    def parents(self) -> "Tuple[bytes, bytes]":
        parents = self._parents
        if not parents:
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
                parents = st[:20], st[20:40]
            elif l == 0:
                parents = (nullid, nullid)
            else:
                raise error.Abort(_("working directory state appears damaged!"))
            self._parents = parents

        return parents

    def setparents(self, p1: bytes, p2: bytes) -> None:
        self._parents = (p1, p2)
        self._dirtyparents = True

    def read(self) -> None:
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
            self._map = parsers.dict_new_presized(len(st) // 71)

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
        self.__contains__: "Callable[[str], bool]" = self._map.__contains__
        self.__getitem__: "Callable[[str], dirstatetuple]" = self._map.__getitem__
        self.get = self._map.get

    def write(self, st: "BinaryIO", now: int) -> None:
        st.write(parsers.pack_dirstate(self._map, self.copymap, self.parents(), now))
        st.close()
        self._dirtyparents = False
        self.nonnormalset, self.otherparentset = self.nonnormalentries()

    @util.propertycache
    def nonnormalset(self) -> "Set[str]":
        nonnorm, otherparents = self.nonnormalentries()
        self.otherparentset = otherparents
        return nonnorm

    @util.propertycache
    def otherparentset(self) -> "Set[str]":
        nonnorm, otherparents = self.nonnormalentries()
        self.nonnormalset = nonnorm
        return otherparents

    @util.propertycache
    def identity(self) -> "util.filestat":
        self._map
        return self.identity

    @util.propertycache
    def dirfoldmap(self) -> "Dict[str, str]":
        f = {}
        normcase = util.normcase
        if "_dirs" in self.__dict__:
            for name in self._dirs:
                f[normcase(name)] = name
        return f
