# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# context.py - changeset and file context objects for mercurial
#
# Copyright 2006, 2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import filecmp
import hashlib
import os
import re
import stat
import sys
from typing import Callable, List, Tuple

from . import (
    annotate,
    encoding,
    error,
    extensions,
    fileset,
    git,
    match as matchmod,
    mutation,
    patch,
    pathutil,
    phases,
    pycompat,
    revlog,
    scmutil,
    util,
    visibility,
)
from .i18n import _
from .node import (
    addednodeid,
    bin,
    hex,
    modifiednodeid,
    nullhex,
    nullid,
    short,
    wdirid,
    wdirnodes,
    wdirrev,
)
from .pycompat import encodeutf8, isint, range
from .thirdparty import attr


propertycache = util.propertycache

nonascii = re.compile(r"[^\x21-\x7f]").search

slowstatuswarning = _(
    "(status will still be slow next time; try to complete or abort other source control operations and then run '@prog@ status' again)\n"
)


class basectx(object):
    """A basectx object represents the common logic for its children:
    changectx: read-only context that is already present in the repo,
    workingctx: a context that represents the working directory and can
                be committed,
    memctx: a context that represents changes in-memory and can also
            be committed."""

    _node = nullid
    _repo = None

    def __new__(cls, repo, changeid="", *args, **kwargs):
        if isinstance(changeid, basectx):
            return changeid

        o = super(basectx, cls).__new__(cls)

        o._repo = repo
        o._node = nullid

        return o

    @property
    def _rev(self):
        if self._node is None:
            # workingctx
            return None
        return self._repo.changelog.rev(self._node)

    def __bytes__(self):
        return encodeutf8(str(self))

    def __str__(self):
        node = self.node()
        if node is not None:
            return short(self.node())
        else:
            return "none"

    def __int__(self):
        return self.rev()

    def __repr__(self):
        return r"<%s %s>" % (type(self).__name__, self)

    def __eq__(self, other):
        try:
            return type(self) == type(other) and self._node == other._node
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def __contains__(self, key):
        return key in self._manifest

    def __getitem__(self, key):
        return self.filectx(key)

    def __iter__(self):
        return iter(self._manifest)

    def buildstatusmanifest(self, status):
        """Builds a manifest that includes the given status results, if this is
        a working copy context. For non-working copy contexts, it just returns
        the normal manifest."""
        return self.manifest()

    def _matchstatus(self, other, match):
        """This internal method provides a way for child objects to override the
        match operator.
        """
        return match

    def _buildstatus(self, other, s, match, listignored, listclean, listunknown):
        """build a status with respect to another context"""
        # Load earliest manifest first for caching reasons. More specifically,
        # if you have revisions 1000 and 1001, 1001 is probably stored as a
        # delta against 1000. Thus, if you read 1000 first, we'll reconstruct
        # 1000 and cache it so that when you read 1001, we just need to apply a
        # delta to what's in the cache. So that's one full reconstruction + one
        # delta application.
        mf2 = None
        if self.rev() is not None and self.rev() < other.rev():
            mf2 = self.buildstatusmanifest(s)
        mf1 = other.buildstatusmanifest(s)
        if mf2 is None:
            mf2 = self.buildstatusmanifest(s)

        modified, added = [], []
        removed = []
        if listclean:
            cleanset = set(mf1.walk(match))
        else:
            cleanset = set()
        deleted, unknown, ignored = s.deleted, s.unknown, s.ignored
        deletedset = set(deleted)
        d = mf1.diff(mf2, matcher=match)
        for fn, value in pycompat.iteritems(d):
            if listclean:
                cleanset.discard(fn)
            if fn in deletedset:
                continue
            (node1, flag1), (node2, flag2) = value
            if node1 is None:
                added.append(fn)
            elif node2 is None:
                removed.append(fn)
            elif flag1 != flag2:
                modified.append(fn)
            elif (
                not self._repo.ui.configbool("scmstore", "status")
                and node2 not in wdirnodes
            ):
                # When comparing files between two commits, we save time by
                # not comparing the file contents when the nodeids differ.
                # Note that this means we incorrectly report a reverted change
                # to a file as a modification.
                # TODO(meyer): Update this comment when remote aux data fetching
                # is implemented.
                # When scmstore is enabled for status we skip this shortcut
                # and instead report accurately, which (once remote aux data
                # fetching is implemented) will no longer require downloading
                # the full file contents just to determine if it's really a
                # modification.
                modified.append(fn)
            elif self[fn].cmp(other[fn]):
                modified.append(fn)
            else:
                cleanset.add(fn)

        if removed:
            # need to filter files if they are already reported as removed
            unknown = [
                fn for fn in unknown if fn not in mf1 and (not match or match(fn))
            ]
            ignored = [
                fn for fn in ignored if fn not in mf1 and (not match or match(fn))
            ]
            # if they're deleted, don't report them as removed
            removed = [fn for fn in removed if fn not in deletedset]
        clean = list(cleanset)

        return scmutil.status(
            modified, added, removed, deleted, unknown, ignored, clean
        )

    def rev(self) -> int:
        return self._rev

    def node(self) -> bytes:
        return self._node

    def hex(self) -> str:
        return hex(self.node())

    def manifest(self):
        return self._manifest

    def manifestctx(self):
        return self._manifestctx

    def repo(self):
        return self._repo

    def phasestr(self) -> str:
        return phases.phasenames[self.phase()]

    def phase(self) -> int:
        raise NotImplementedError()

    def mutable(self) -> bool:
        return self.phase() > phases.public

    def getfileset(self, expr):
        return fileset.getfileset(self, expr)

    def invisible(self) -> bool:
        repo = self.repo()
        if visibility.enabled(repo):
            return self.rev() in visibility.invisiblerevs(repo)
        else:
            return False

    def obsolete(self) -> bool:
        """True if the changeset is obsolete"""
        if mutation.enabled(self._repo):
            return mutation.isobsolete(self._repo, self.node())
        else:
            return False

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def p1(self):
        p = self.parents()
        if p:
            return p[0]
        return changectx(self._repo, nullid)

    def p2(self):
        parents = self._parents
        if len(parents) == 2:
            return parents[1]
        return changectx(self._repo, nullid)

    def _fileinfo(self, path):
        if r"_manifest" in self.__dict__:
            try:
                return self._manifest.find(path)
            except KeyError:
                raise error.ManifestLookupError(
                    self._node, path, _("not found in manifest")
                )
        mfl = self._repo.manifestlog
        try:
            node, flag = mfl[self._changeset.manifest].find(path)
        except KeyError:
            raise error.ManifestLookupError(
                self._node, path, _("not found in manifest")
            )

        return node, flag

    def filenode(self, path):
        return self._fileinfo(path)[0]

    def flags(self, path):
        try:
            return self._fileinfo(path)[1]
        except error.LookupError:
            return ""

    def match(
        self,
        pats=None,
        include=None,
        exclude=None,
        default="glob",
        badfn=None,
        emptyalways=True,
        warn=None,
    ):
        r = self._repo
        return matchmod.match(
            r.root,
            r.getcwd(),
            pats,
            include,
            exclude,
            default,
            auditor=r.nofsauditor,
            ctx=self,
            badfn=badfn,
            emptyalways=emptyalways,
            warn=warn,
        )

    def diff(self, ctx2=None, match=None, **opts):
        """Returns a diff generator for the given contexts and matcher"""
        if ctx2 is None:
            ctx2 = self.p1()
        if ctx2 is not None:
            ctx2 = self._repo[ctx2]
        diffopts = patch.diffopts(self._repo.ui, opts)
        return patch.diff(self._repo, ctx2, self, match=match, opts=diffopts)

    def dirs(self):
        return util.dirs(self._manifest)

    def hasdir(self, dir):
        return self._manifest.hasdir(dir)

    def status(
        self,
        other=None,
        match=None,
        listignored=False,
        listclean=False,
        listunknown=False,
    ):
        """return status of files between two nodes or node and working
        directory.

        If other is None, compare this node with working directory.

        returns (modified, added, removed, deleted, unknown, ignored, clean)
        """

        ctx1 = self
        ctx2 = self._repo[other]

        # This next code block is, admittedly, fragile logic that tests for
        # reversing the contexts and wouldn't need to exist if it weren't for
        # the fast (and common) code path of comparing the working directory
        # with its first parent.
        #
        # What we're aiming for here is the ability to call:
        #
        # workingctx.status(parentctx)
        #
        # If we always built the manifest for each context and compared those,
        # then we'd be done. But the special case of the above call means we
        # just copy the manifest of the parent.
        reversed = False
        if not isinstance(ctx1, changectx) and isinstance(ctx2, changectx):
            reversed = True
            ctx1, ctx2 = ctx2, ctx1

        match = match or matchmod.always(self._repo.root, self._repo.getcwd())
        match = ctx2._matchstatus(ctx1, match)
        r = scmutil.status([], [], [], [], [], [], [])
        r = ctx2._buildstatus(ctx1, r, match, listignored, listclean, listunknown)

        if reversed:
            # Reverse added and removed. Clear deleted, unknown and ignored as
            # these make no sense to reverse.
            r = scmutil.status(r.modified, r.removed, r.added, [], [], [], r.clean)

        for l in r:
            l.sort()

        return r


class changectx(basectx):
    """A changecontext object makes access to data related to a particular
    changeset convenient. It represents a read-only context already present in
    the repo."""

    def __init__(self, repo, changeid=""):
        """changeid is a revision number or node"""

        # since basectx.__new__ already took care of copying the object, we
        # don't need to do anything in __init__, so we just exit here
        if isinstance(changeid, basectx):
            return

        if changeid == "":
            changeid = "."
        self._repo = repo

        try:
            if isint(changeid):
                changeid = scmutil.revf64decode(changeid)
                self._node = repo.changelog.node(changeid)
                return
            if changeid == "null":
                self._node = nullid
                return
            if changeid == "tip":
                self._node = repo.changelog.tip()
                return
            try:
                if changeid == "." or repo.local() and changeid == repo.dirstate.p1():
                    # this is a hack to delay/avoid loading obsmarkers
                    # when we know that '.' won't be hidden
                    self._node = repo.dirstate.p1()
                    return
            except Exception:
                if not getattr(self._repo, "_warnedworkdir", False):
                    self._repo.ui.warn(
                        _("warning: failed to inspect working copy parent\n")
                    )
                    self._repo._warnedworkdir = True
                # we failed on our optimization pass
                # this can happen when dirstate is broken
            if len(changeid) == 20 and isinstance(changeid, bytes):
                try:
                    self._node = changeid
                    repo.changelog.rev(changeid)
                    return
                except LookupError:
                    # The only valid bytes changeid is a node, and if the node was not
                    # found above, this is now considered an unknown changeid.
                    # let's convert, or go ahead and through.
                    if sys.version_info[0] >= 3 and isinstance(changeid, bytes):
                        # Hex the node so it prints pretty.
                        changeid = hex(changeid)
                        raise error.RepoLookupError(
                            _("unknown revision '%s'") % changeid
                        )

            # The valid changeid types are str, bytes, and int. int and bytes
            # are handled above, so only str should be present now.
            assert isinstance(changeid, str)

            # Try to resolve it as a rev number?
            # - If changeid is an int (tested above).
            # - If HGPLAIN is set (for compatibility).
            # - Or if ui.ignorerevnum is false (changeid is a str).
            if repo.ui.plain() or not repo.ui.configbool("ui", "ignorerevnum"):
                try:
                    r = int(changeid)
                    if "%d" % r != changeid:
                        raise ValueError
                    if r < 0 and r != wdirrev:
                        if -r > len(repo):
                            raise ValueError
                        r = repo.revs("first(sort(_all(), -rev), %z)", -r).last()
                        if r is None:
                            raise ValueError
                    if r < 0 and r != wdirrev:
                        raise ValueError
                    r = scmutil.revf64decode(r)
                    node = repo.changelog.node(r)
                    self._node = node
                    return
                except (
                    ValueError,
                    OverflowError,
                    IndexError,
                    TypeError,
                    error.RustError,
                ):
                    pass

            if len(changeid) == 40:
                try:
                    self._node = bin(changeid)
                    repo.changelog.rev(self._node)
                    return
                except (TypeError, LookupError):
                    pass

            # lookup bookmarks through the name interface
            try:
                self._node = repo.names.singlenode(repo, changeid)
                repo.changelog.rev(self._node)
                return
            except KeyError:
                pass
            except error.RepoLookupError:
                pass

            self._node = repo.changelog._partialmatch(changeid)
            if self._node is not None:
                repo.changelog.rev(self._node)
                return

            # lookup failed
            # check if it might have come from damaged dirstate
            if repo.local() and changeid in repo.dirstate.parents():
                msg = _("working directory has unknown parent '%s'!")
                raise error.Abort(msg % short(changeid))
            try:
                if len(changeid) == 20 and nonascii(changeid):
                    changeid = hex(changeid)
            except TypeError:
                pass
        except IndexError:
            pass
        raise error.RepoLookupError(_("unknown revision '%s'") % changeid)

    def __hash__(self):
        try:
            return hash(self._node)
        except AttributeError:
            return id(self)

    def __nonzero__(self):
        return self._node != nullid

    __bool__ = __nonzero__

    @propertycache
    def _changeset(self):
        return self._repo.changelog.changelogrevision(self._node)

    @propertycache
    def _manifest(self):
        self._repo.manifestlog.recentlinknode = self.node()
        return self._manifestctx.read()

    @property
    def _manifestctx(self):
        self._repo.manifestlog.recentlinknode = self.node()
        try:
            return self._repo.manifestlog[self._changeset.manifest]
        except Exception as ex:
            error.addcontext(ex, lambda: _("(commit: %s)") % self.hex())
            raise

    @propertycache
    def _parents(self):
        repo = self._repo
        pnodes = repo.changelog.parents(self._node, fillnullid=False)
        return [changectx(repo, p) for p in pnodes]

    def changeset(self):
        c = self._changeset
        return (c.manifest, c.user, c.date, c.files, c.description, c.extra)

    def manifestnode(self):
        return self._changeset.manifest

    def user(self):
        return self._changeset.user

    def date(self):
        return self._changeset.date

    def files(self):
        files = self._changeset.files
        # git does not provide "files" in commit message - run diff to get it
        if not files and git.isgitformat(self._repo):
            files = sorted(self.manifest().diff(self.p1().manifest()).keys())
        return files

    def description(self):
        return self._changeset.description

    def shortdescription(self):
        return self.description().splitlines()[0]

    def branch(self):
        return encoding.tolocal(self._changeset.extra.get("branch"))

    def closesbranch(self):
        return "close" in self._changeset.extra

    def extra(self):
        """Return a dict of extra information."""
        return self._changeset.extra

    def bookmarks(self):
        """Return a list of byte bookmark names."""
        return self._repo.nodebookmarks(self._node)

    def phase(self) -> int:
        return self._repo._phasecache.phase(self._repo, self._rev)

    @propertycache
    def _mutationentry(self):
        return mutation.lookup(self._repo, self._node)

    def mutationpredecessors(self):
        if self._mutationentry:
            return self._mutationentry.preds()

    def mutationoperation(self):
        if self._mutationentry:
            return self._mutationentry.op()

    def mutationuser(self):
        if self._mutationentry:
            return self._mutationentry.user()

    def mutationdate(self):
        if self._mutationentry:
            return (self._mutationentry.time(), self._mutationentry.tz())

    def mutationsplit(self):
        if self._mutationentry:
            return self._mutationentry.split()

    def isinmemory(self):
        return False

    def children(self):
        """return list of changectx contexts for each child changeset.

        This returns only the immediate child changesets. Use descendants() to
        recursively walk children.
        """
        c = self._repo.changelog.children(self._node)
        children = [changectx(self._repo, x) for x in c]
        return [ctx for ctx in children if ctx.phase() != phases.secret]

    def ancestors(self):
        for a in self._repo.changelog.ancestors([self._rev]):
            yield changectx(self._repo, a)

    def descendants(self):
        """Recursively yield all children of the changeset.

        For just the immediate children, use children()
        """
        for d in self._repo.changelog.descendants([self._rev]):
            yield changectx(self._repo, d)

    def filectx(self, path, fileid=None, filelog=None):
        """get a file context from this changeset"""
        if fileid is None:
            fileid = self.filenode(path)
        return filectx(self._repo, path, fileid=fileid, changectx=self, filelog=filelog)

    def ancestor(self, c2, warn=False):
        """return the "best" ancestor context of self and c2

        If there are multiple candidates, it will show a message and check
        merge.preferancestor configuration before falling back to the
        revlog ancestor."""
        # deal with workingctxs
        n2 = c2._node
        if n2 is None:
            n2 = c2._parents[0]._node
        cahs = self._repo.changelog.commonancestorsheads(self._node, n2)
        if not cahs:
            anc = nullid
        elif len(cahs) == 1:
            anc = cahs[0]
        else:
            # experimental config: merge.preferancestor
            for r in self._repo.ui.configlist("merge", "preferancestor"):
                try:
                    ctx = changectx(self._repo, r)
                except error.RepoLookupError:
                    continue
                anc = ctx.node()
                if anc in cahs:
                    break
            else:
                anc = self._repo.changelog.ancestor(self._node, n2)
            if warn:
                self._repo.ui.status(
                    (
                        _("note: using %s as ancestor of %s and %s\n")
                        % (short(anc), short(self._node), short(n2))
                    )
                    + "".join(
                        _(
                            "      alternatively, use --config "
                            "merge.preferancestor=%s\n"
                        )
                        % short(n)
                        for n in sorted(cahs)
                        if n != anc
                    )
                )
        return changectx(self._repo, anc)

    def descendant(self, other):
        """True if other is descendant of this changeset"""
        return self._repo.changelog.descendant(self._rev, other._rev)

    def walk(self, match):
        """Generates matching file names."""

        # Wrap match.bad method to have message with nodeid
        def bad(fn, msg):
            match.bad(fn, _("no such file in rev %s") % self)

        m = matchmod.badmatch(match, bad)
        return self._manifest.walk(m)

    def matches(self, match):
        return self.walk(match)


class basefilectx(object):
    """A filecontext object represents the common logic for its children:
    filectx: read-only access to a filerevision that is already present
             in the repo,
    workingfilectx: a filecontext that represents files from the working
                    directory,
    memfilectx: a filecontext that represents files in-memory,
    overlayfilectx: duplicate another filecontext with some fields overridden.
    """

    @propertycache
    def _filelog(self):
        return self._repo.file(self._path)

    @propertycache
    def _changeid(self):
        if r"_changeid" in self.__dict__:
            return self._changeid
        elif r"_changectx" in self.__dict__:
            return self._changectx.rev()
        elif r"_descendantrev" in self.__dict__:
            # this file context was created from a revision with a known
            # descendant, we can (lazily) correct for linkrev aliases
            return self._adjustlinkrev(self._descendantrev)
        else:
            return self._filelog.linkrev(self._filerev)

    @propertycache
    def _filenode(self):
        if r"_fileid" in self.__dict__:
            return self._filelog.lookup(self._fileid)
        else:
            return self._changectx.filenode(self._path)

    @propertycache
    def _filerev(self):
        return self._filelog.rev(self._filenode)

    @propertycache
    def _repopath(self):
        return self._path

    def __nonzero__(self):
        try:
            self._filenode
            return True
        except error.LookupError:
            # file is missing
            return False

    __bool__ = __nonzero__

    def __bytes__(self):
        return encodeutf8(str(self))

    def __str__(self):
        try:
            return "%s@%s" % (self.path(), self._changectx)
        except error.LookupError:
            return "%s@???" % self.path()

    def __repr__(self):
        return "<%s %s>" % (type(self).__name__, str(self))

    def __hash__(self):
        try:
            return hash((self._path, self._filenode))
        except AttributeError:
            return id(self)

    def __eq__(self, other):
        try:
            # Traditional hg, filenode includes history.
            eq1 = (
                type(self) == type(other)
                and self._path == other._path
                and self._filenode == other._filenode
            )
            if not eq1:
                return False
            # For Git, also check the commit hash. `.node()` might trigger some
            # calculations so we only do it when the above eq is not decisive.
            return self.node() == other.node()
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def filerev(self):
        return self._filerev

    def filenode(self):
        return self._filenode

    @propertycache
    def _flags(self):
        return self._changectx.flags(self._path)

    def flags(self):
        return self._flags

    def filelog(self):
        return self._filelog

    def rev(self):
        return self._changeid

    def linkrev(self):
        if "invalidatelinkrev" in self._repo.storerequirements:
            return None
        return self._filelog.linkrev(self._filerev)

    def node(self):
        return self._changectx.node()

    def hex(self):
        return self._changectx.hex()

    def user(self):
        return self._changectx.user()

    def date(self):
        return self._changectx.date()

    def files(self):
        return self._changectx.files()

    def description(self):
        return self._changectx.description()

    def branch(self):
        return self._changectx.branch()

    def extra(self):
        return self._changectx.extra()

    def phase(self):
        return self._changectx.phase()

    def phasestr(self):
        return self._changectx.phasestr()

    def obsolete(self):
        return self._changectx.obsolete()

    def manifest(self):
        return self._changectx.manifest()

    def changectx(self):
        return self._changectx

    def renamed(self):
        return self._copied

    def repo(self):
        return self._repo

    def size(self):
        return len(self.data())

    def path(self):
        return self._path

    def content_sha256(self):
        if extensions.isenabled(
            self._repo.ui, "remotefilelog"
        ) and self._repo.ui.configbool("scmstore", "status"):
            return self._repo.fileslog.filescmstore.fetch_contentsha256(
                [(self.path(), self.filenode())]
            )[0][1]
        return hashlib.sha256(self.data()).digest()

    def isbinary(self):
        try:
            return util.binary(self.data())
        except IOError:
            return False

    def isexec(self):
        return "x" in self.flags()

    def islink(self):
        return "l" in self.flags()

    def isabsent(self):
        """whether this filectx represents a file not in self._changectx

        This is mainly for merge code to detect change/delete conflicts. This is
        expected to be True for all subclasses of basectx."""
        return False

    _customcmp = False

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        if fctx._customcmp:
            return fctx.cmp(self)

        if (
            fctx._filenode is None
            and (
                self._repo._encodefilterpats
                # if file data starts with '\1\n', empty metadata block is
                # prepended, which adds 4 bytes to filelog.size().
                or self.size() - 4 == fctx.size()
            )
            or self.size() == fctx.size()
        ):
            if self._filenode is None:
                # Both self and fctx are in-memory. Do a content check.
                # PERF: This might be improved for LFS cases.
                return self.data() != fctx.data()
            if self._repo.ui.configbool("scmstore", "status"):
                return self.content_sha256() != fctx.content_sha256()
            return self._filelog.cmp(self._filenode, fctx.data())

        return True

    def _adjustlinkrev(self, srcrev, inclusive=False):
        """return the first ancestor of <srcrev> introducing <fnode>

        If the linkrev of the file revision does not point to an ancestor of
        srcrev, we'll walk down the ancestors until we find one introducing
        this file revision.

        :srcrev: the changeset revision we search ancestors from
        :inclusive: if true, the src revision will also be checked
        """
        repo = self._repo
        cl = repo.changelog
        mfl = repo.manifestlog
        # fetch the linkrev
        lkr = self.linkrev()
        # developer config: unsafe.incorrectfilehistory
        if lkr is not None and repo.ui.configbool("unsafe", "incorrectfilehistory"):
            return lkr
        # hack to reuse ancestor computation when searching for renames
        memberanc = getattr(self, "_ancestrycontext", None)
        iteranc = None
        if srcrev is None:
            # wctx case, used by workingfilectx during mergecopy
            revs = [p.rev() for p in self._repo[None].parents()]
            inclusive = True  # we skipped the real (revless) source
        else:
            revs = [srcrev]
        if memberanc is None:
            memberanc = iteranc = cl.ancestors(revs, lkr or 0, inclusive=inclusive)
        # check if this linkrev is an ancestor of srcrev
        if lkr is None or lkr not in memberanc:
            if iteranc is None:
                iteranc = cl.ancestors(revs, lkr or 0, inclusive=inclusive)
            fnode = self._filenode
            path = self._path
            for a in iteranc:
                ac = cl.read(a)  # get changeset data (we avoid object creation)
                if path in ac[3]:  # checking the 'files' field.
                    # The file has been touched, check if the content is
                    # similar to the one we search for.
                    if fnode == mfl[ac[0]].read().get(path):
                        return a
            # In theory, we should never get out of that loop without a result.
            # But if manifest uses a buggy file revision (not children of the
            # one it replaces) we could. Such a buggy situation will likely
            # result is crash somewhere else at to some point.
        assert lkr is not None
        return lkr

    def introrev(self):
        """return the rev of the changeset which introduced this file revision

        This method is different from linkrev because it take into account the
        changeset the filectx was created from. It ensures the returned
        revision is one of its ancestors. This prevents bugs from
        'linkrev-shadowing' when a file revision is used by multiple
        changesets.
        """
        lkr = self.linkrev()
        attrs = vars(self)
        noctx = not ("_changeid" in attrs or "_changectx" in attrs)
        if noctx or self.rev() == lkr:
            return self.linkrev()
        return self._adjustlinkrev(self.rev(), inclusive=True)

    def introfilectx(self):
        """Return filectx having identical contents, but pointing to the
        changeset revision where this filectx was introduced"""
        introrev = self.introrev()
        if self.rev() == introrev:
            return self
        return self.filectx(self.filenode(), changeid=introrev)

    def _parentfilectx(self, path, fileid, filelog):
        """create parent filectx keeping ancestry info for _adjustlinkrev()"""
        fctx = filectx(self._repo, path, fileid=fileid, filelog=filelog)
        if "_changeid" in vars(self) or "_changectx" in vars(self):
            # If self is associated with a changeset (probably explicitly
            # fed), ensure the created filectx is associated with a
            # changeset that is an ancestor of self.changectx.
            # This lets us later use _adjustlinkrev to get a correct link.
            fctx._descendantrev = self.rev()
            fctx._ancestrycontext = getattr(self, "_ancestrycontext", None)
        elif "_descendantrev" in vars(self):
            # Otherwise propagate _descendantrev if we have one associated.
            fctx._descendantrev = self._descendantrev
            fctx._ancestrycontext = getattr(self, "_ancestrycontext", None)
        return fctx

    def parents(self):
        _path = self._path
        fl = self._filelog
        parents = self._filelog.parents(self._filenode)
        pl = [(_path, node, fl) for node in parents if node != nullid]

        r = fl.renamed(self._filenode)
        if r:
            # - In the simple rename case, both parent are nullid, pl is empty.
            # - In case of merge, only one of the parent is null id and should
            # be replaced with the rename information. This parent is -always-
            # the first one.
            #
            # As null id have always been filtered out in the previous list
            # comprehension, inserting to 0 will always result in "replacing
            # first nullid parent with rename information.
            pl.insert(0, (r[0], r[1], self._repo.file(r[0])))

        return [self._parentfilectx(path, fnode, l) for path, fnode, l in pl]

    def p1(self):
        return self.parents()[0]

    def p2(self):
        p = self.parents()
        if len(p) == 2:
            return p[1]
        return filectx(self._repo, self._path, fileid=nullid, filelog=self._filelog)

    def annotate(self, follow=False, linenumber=False, skiprevs=None, diffopts=None):
        """returns a list of tuples of ((ctx, number), line) for each line
        in the file, where ctx is the filectx of the node where
        that line was last changed; if linenumber parameter is true, number is
        the line number at the first appearance in the managed file, otherwise,
        number has a fixed value of False.
        """

        def lines(text):
            if text.endswith(b"\n"):
                return text.count(b"\n")
            return text.count(b"\n") + int(bool(text))

        if linenumber:

            def decorate(fctx):
                text = fctx.data()
                return (
                    [
                        annotateline(fctx=fctx, lineno=i)
                        for i in range(1, lines(text) + 1)
                    ],
                    text,
                )

        else:

            def decorate(fctx):
                text = fctx.data()
                return ([annotateline(fctx=fctx)] * lines(text), text)

        if skiprevs is not None:

            def skipfunc(fctx, skiprevs=skiprevs):
                return fctx._changeid in skiprevs

        else:
            skipfunc = None

        repo = self.repo()
        if git.isgitstore(repo) or repo.ui.configbool("experimental", "pathhistory"):
            # git does not have filelog to answer history questions
            base, parents = _pathhistorybaseparents(self, follow)
        else:
            base, parents = _filelogbaseparents(self, follow)

        annotatedlines, text = annotate.annotate(
            base, parents, decorate, diffopts, skipfunc
        )
        return zip(annotatedlines, text.splitlines(True))

    def topologicalancestors(self, followfirst=False):
        return self.ancestors(followfirst=followfirst)

    def ancestors(self, followfirst=False):
        visit = {}
        c = self
        if followfirst:
            cut = 1
        else:
            cut = None

        while True:
            for parent in c.parents()[:cut]:
                visit[(parent.linkrev(), parent.filenode())] = parent
            if not visit:
                break
            c = visit.pop(max(visit))
            yield c

    def decodeddata(self):
        """Returns `data()` after running repository decoding filters.

        This is often equivalent to how the data would be expressed on disk.
        """
        return self._repo.wwritedata(self.path(), self.data())


def _filelogbaseparents(
    fctx: basefilectx, follow: bool
) -> Tuple[basefilectx, Callable[[basefilectx], List[basefilectx]]]:
    """Return (base, parents) useful for annotate history traversal.
    This implementation is based on filelog.
    """
    # pyre-fixme[16]: `basefilectx` has no attribute `_repo`.
    getlog = util.lrucachefunc(lambda x: fctx._repo.file(x))

    def parents(f):
        # Cut _descendantrev here to mitigate the penalty of lazy linkrev
        # adjustment. Otherwise, p._adjustlinkrev() would walk changelog
        # from the topmost introrev (= srcrev) down to p.linkrev() if it
        # isn't an ancestor of the srcrev.
        f._changeid
        pl = f.parents()

        # Don't return renamed parents if we aren't following.
        if not follow:
            pl = [p for p in pl if p.path() == f.path()]

        # renamed filectx won't have a filelog yet, so set it
        # from the cache to save time
        for p in pl:
            if not "_filelog" in p.__dict__:
                p._filelog = getlog(p.path())

        return pl

    # use linkrev to find the first changeset where self appeared
    base = fctx.introfilectx()
    if getattr(base, "_ancestrycontext", None) is None:
        cl = fctx._repo.changelog
        if base.rev() is None:
            # wctx is not inclusive, but works because _ancestrycontext
            # is used to test filelog revisions
            ac = cl.ancestors([p.rev() for p in base.parents()], inclusive=True)
        else:
            ac = cl.ancestors([base.rev()], inclusive=True)
        base._ancestrycontext = ac

    return base, parents


def _pathhistorybaseparents(
    fctx: basefilectx, follow: bool
) -> Tuple[basefilectx, Callable[[basefilectx], List[basefilectx]]]:
    """Return (base, parents) useful for annotate history traversal.
    This implementation is based on pathhistory.
    """
    cache = {}  # {path: pathhistoryparents}

    repo = fctx.repo()
    path = fctx.path()
    pathparents = pathhistoryparents(repo, path)
    intronode = pathparents.follow(fctx.node())
    cache[path] = pathparents
    base = repo[intronode][path]

    def parents(fctx, repo=repo, cache=cache):
        path = fctx.path()
        node = fctx.node()
        parent_fctxs = []
        while True:
            parentnodes = cache[path](node)
            absent_nodes = []
            for n in parentnodes:
                pctx = repo[n]
                if path in pctx:
                    pfctx = pctx[path]
                    parent_fctxs.append(pfctx)
                else:
                    absent_nodes.append(n)

            if not parent_fctxs and len(absent_nodes) == 1:
                # If a file was deleted, then re-added, try following its
                # history before the deletion.
                node = absent_nodes[0]
                continue
            break

        # TODO: Consider following renames.
        return parent_fctxs

    return base, parents


class pathhistoryparents:
    """parents for a sub-graph following (path, startnode)"""

    def __init__(self, repo, path: str):
        self.repo = repo
        self.path = path

        self.followed = {}

        # [(nameset, parents)].
        # If a file is renamed forth and back, there might need to be multiple
        # pathhistory follows.
        self.setparents = []

    def follow(self, startnode):
        """follow a node so history starting from that node is known
        Return the 'intronode' - nearest node that touches the path.
        Return nullid if the history is empty.
        The 'intronode' can then be used in '__call__' to get parent
        nodes.
        """
        intronode = self.followed.get(startnode)
        if intronode:
            return intronode
        dag = self.repo.changelog.dag
        ancestornodes = dag.ancestors([startnode])
        nodes = list(self.repo.pathhistory([self.path], ancestornodes))
        nameset = dag.sort(nodes)
        parents = nameset.toparents()
        self.setparents.append((nameset, parents))
        intronode = nodes and nodes[0] or nullid
        self.followed[startnode] = intronode
        return intronode

    def __call__(self, node):
        """get parent nodes of node for path."""
        if node == nullid:
            return []
        for (nameset, parents) in self.setparents:
            if node in nameset:
                return parents(node)
        raise error.ProgrammingError("%s is not yet follow()-ed" % hex(node))


@attr.s(slots=True, frozen=True)
class annotateline(object):
    fctx = attr.ib()
    lineno = attr.ib(default=False)
    # Whether this annotation was the result of a skip-annotate.
    skip = attr.ib(default=False)


class filectx(basefilectx):
    """A filecontext object makes access to data related to a particular
    filerevision convenient."""

    def __init__(
        self, repo, path, changeid=None, fileid=None, filelog=None, changectx=None
    ):
        """changeid can be a changeset revision or node.
        fileid can be a file revision or node."""
        self._repo = repo
        self._path = path

        assert (
            changeid is not None or fileid is not None or changectx is not None
        ), "bad args: changeid=%r, fileid=%r, changectx=%r" % (
            changeid,
            fileid,
            changectx,
        )

        if filelog is not None:
            self._filelog = filelog

        if changeid is not None:
            self._changeid = changeid
        if changectx is not None:
            self._changectx = changectx
        if fileid is not None:
            self._fileid = fileid

    @propertycache
    def _changectx(self):
        if self._changeid is None:
            return workingctx(self._repo)
        return changectx(self._repo, self._changeid)

    def filectx(self, fileid, changeid=None):
        """opens an arbitrary revision of the file without
        opening a new filelog"""
        return filectx(
            self._repo,
            self._path,
            fileid=fileid,
            filelog=self._filelog,
            changeid=changeid,
        )

    def rawdata(self):
        return self._filelog.revision(self._filenode, raw=True)

    def rawflags(self):
        """low-level revlog flags"""
        return self._filelog.flags(self._filerev)

    def data(self) -> bytes:
        if self.flags() == "m":
            text = "Subproject commit %s\n" % hex(self._filenode)
            return text.encode("utf-8")
        try:
            return self._filelog.read(self._filenode)
        except error.CensoredNodeError:
            if self._repo.ui.config("censor", "policy") == "ignore":
                return b""
            raise error.Abort(
                _("censored node: %s") % short(self._filenode),
                hint=_("set censor.policy to ignore errors"),
            )

    def size(self) -> int:
        try:
            return self._filelog.size(self._filerev)
        except error.RustError:
            # For submodule, this raises "object not found" error.
            # Let's just return a dummy size.
            if self.flags() == "m":
                return 0
            raise

    @propertycache
    def _copied(self):
        """check if file was actually renamed in this changeset revision

        If rename logged in file revision, we report copy for changeset only
        if file revisions linkrev points back to the changeset in question
        or both changeset parents contain different file revisions.
        """

        renamed = self._filelog.renamed(self._filenode)
        if not renamed:
            return renamed

        if self.rev() == self.linkrev():
            return renamed

        name = self.path()
        fnode = self._filenode
        for p in self._changectx.parents():
            try:
                if fnode == p.filenode(name):
                    return None
            except error.LookupError:
                pass
        return renamed

    def children(self):
        # hard for renames
        c = self._filelog.children(self._filenode)
        return [
            filectx(self._repo, self._path, fileid=x, filelog=self._filelog) for x in c
        ]


class committablectx(basectx):
    """A committablectx object provides common functionality for a context that
    wants the ability to commit, e.g. workingctx or memctx."""

    def __init__(
        self,
        repo,
        text="",
        user=None,
        date=None,
        extra=None,
        changes=None,
        loginfo=None,
        mutinfo=None,
    ):
        self._repo = repo
        self._node = None
        self._text = text
        if date:
            self._date = util.parsedate(date)
        if user:
            self._user = user
        if changes:
            self._status = changes
        self._loginfo = loginfo
        self._mutinfo = mutinfo

        self._extra = {}
        if extra:
            self._extra = extra.copy()
        if "branch" not in self._extra:
            try:
                branch = encoding.fromlocal(self._repo.dirstate.branch())
            except UnicodeDecodeError:
                raise error.Abort(_("branch name not in UTF-8!"))
            self._extra["branch"] = branch
        if self._extra["branch"] == "":
            self._extra["branch"] = "default"

    def __bytes__(self):
        return encodeutf8(str(self))

    def __str__(self):
        return str(self._parents[0]) + "+"

    def __nonzero__(self):
        return True

    __bool__ = __nonzero__

    def _buildflagfunc(self):
        # Create a fallback function for getting file flags when the
        # filesystem doesn't support them

        copiesget = self._repo.dirstate.copies().get
        parents = self.parents()
        if len(parents) < 2:
            # when we have one parent, it's easy: copy from parent
            man = parents[0].manifest()

            def func(f):
                f = copiesget(f, f)
                return man.flags(f)

        else:
            # merges are tricky: we try to reconstruct the unstored
            # result from the merge (issue1802)
            p1, p2 = parents
            pa = p1.ancestor(p2)
            m1, m2, ma = p1.manifest(), p2.manifest(), pa.manifest()

            def func(f):
                f = copiesget(f, f)  # may be wrong for merges with copies
                fl1, fl2, fla = m1.flags(f), m2.flags(f), ma.flags(f)
                if fl1 == fl2:
                    return fl1
                if fl1 == fla:
                    return fl2
                if fl2 == fla:
                    return fl1
                return ""  # punt for conflicts

        return func

    @propertycache
    def _flagfunc(self):
        func = self._repo.dirstate.flagfunc(self._buildflagfunc)
        if git.isgitformat(self._repo):
            # change submodule flags to 'm'
            submodules = git.parsesubmodules(self)
            if submodules:
                submodulepaths = set(m.path for m in submodules)

                def flagfunc(path, orig=func, submodulepaths=submodulepaths):
                    if path in submodulepaths:
                        return "m"
                    else:
                        return orig(path)

                func = flagfunc
        return func

    @propertycache
    def _status(self):
        return self._repo.status()

    @propertycache
    def _user(self):
        return self._repo.ui.username()

    @propertycache
    def _date(self):
        ui = self._repo.ui
        date = ui.configdate("devel", "default-date")
        if date is None:
            date = util.makedate()
        return date

    def manifestnode(self):
        return None

    def user(self):
        return self._user or self._repo.ui.username()

    def date(self):
        return self._date

    def description(self):
        return self._text

    def files(self):
        return sorted(self._status.modified + self._status.added + self._status.removed)

    def modified(self):
        return self._status.modified

    def added(self):
        return self._status.added

    def removed(self):
        return self._status.removed

    def deleted(self):
        return self._status.deleted

    def branch(self):
        return encoding.tolocal(self._extra["branch"])

    def closesbranch(self):
        return "close" in self._extra

    def extra(self):
        return self._extra

    def isinmemory(self):
        return False

    def bookmarks(self):
        b = []
        for p in self.parents():
            b.extend(p.bookmarks())
        return b

    def phase(self) -> int:
        phase = phases.draft  # default phase to draft
        for p in self.parents():
            phase = max(phase, p.phase())
        return phase

    def hidden(self):
        return False

    def children(self):
        return []

    def flags(self, path):
        if r"_manifest" in self.__dict__:
            try:
                return self._manifest.flags(path)
            except KeyError:
                return ""

        try:
            return self._flagfunc(path)
        except OSError:
            return ""

    def ancestor(self, c2):
        """return the "best" ancestor context of self and c2"""
        return self._parents[0].ancestor(c2)  # punt on two parents for now

    def walk(self, match):
        """Generates matching file names."""
        repo = self._repo
        dirstate = repo.dirstate
        status = dirstate.status(match, False, True, True)
        files = set(file for files in status for file in files)

        # It's expensive to ask status to return ignored files, and we only care
        # about it for explicitly mentioned files, so let's manually check them.
        # Also check for explicitly requested files. There's legacy behavior
        # where walk is expected to return explicitly requested files, even if
        # the provided matcher says we shouldn't visit the directories leading
        # to the explicit file (ex: `hg debugwalk -Xbeans beans/black` should
        # show beans/black).
        ignored = dirstate._ignore
        files.update(
            file
            for file in match.files()
            if file in self or (ignored(file) and repo.wvfs.isfileorlink(file))
        )
        return sorted(files)

    def matches(self, match):
        # XXX: Consider using: return sorted(self._manifest.walk(match))
        return sorted(self._repo.dirstate.matches(match))

    def ancestors(self):
        for p in self._parents:
            yield p
        for a in self._repo.changelog.ancestors([p.rev() for p in self._parents]):
            yield changectx(self._repo, a)

    def markcommitted(self, node):
        """Perform post-commit cleanup necessary after committing this ctx

        Specifically, this updates backing stores this working context
        wraps to reflect the fact that the changes reflected by this
        workingctx have been committed.  For example, it marks
        modified and added files as normal in the dirstate.

        """

        with self._repo.dirstate.parentchange():
            for f in self.modified() + self.added():
                self._repo.dirstate.normal(f)
            for f in self.removed():
                self._repo.dirstate.delete(f)
            self._repo.dirstate.setparents(node)

        # write changes out explicitly, because nesting wlock at
        # runtime may prevent 'wlock.release()' in 'repo.commit()'
        # from immediately doing so for subsequent changing files
        self._repo.dirstate.write(self._repo.currenttransaction())

    def dirty(self, missing=False, merge=True, branch=True):
        return False

    def loginfo(self):
        return self._loginfo or {}

    def mutinfo(self):
        return self._mutinfo


class workingctx(committablectx):
    """A workingctx object makes access to data related to
    the current working directory convenient.
    date - any valid date string or (unixtime, offset), or None.
    user - username string, or None.
    extra - a dictionary of extra values, or None.
    changes - a list of file lists as returned by localrepo.status()
               or None to use the repository status.
    """

    def __init__(
        self,
        repo,
        text="",
        user=None,
        date=None,
        extra=None,
        changes=None,
        loginfo=None,
        mutinfo=None,
    ):
        super(workingctx, self).__init__(
            repo, text, user, date, extra, changes, loginfo, mutinfo
        )

    def __iter__(self):
        d = self._repo.dirstate
        for f in d:
            if d[f] != "r":
                yield f

    def __contains__(self, key):
        return self._repo.dirstate[key] not in "?r"

    def hex(self) -> str:
        return hex(wdirid)

    @propertycache
    def _parents(self):
        p = self._repo.dirstate.parents()
        if p[1] == nullid:
            p = p[:-1]
        return [changectx(self._repo, x) for x in p]

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return workingfilectx(self._repo, path, workingctx=self, filelog=filelog)

    def dirty(self, missing=False, merge=True, branch=True):
        "check whether a working directory is modified"
        # check current working dir
        return (
            (merge and self.p2())
            or (branch and self.branch() != self.p1().branch())
            or self.modified()
            or self.added()
            or self.removed()
            or (missing and self.deleted())
        )

    def add(self, list, prefix="", quiet=False):
        with self._repo.wlock():
            ui, ds = self._repo.ui, self._repo.dirstate
            uipath = lambda f: ds.pathto(pathutil.join(prefix, f))
            rejected = []
            lstat = self._repo.wvfs.lstat
            for f in list:
                # ds.pathto() returns an absolute file when this is invoked from
                # the keyword extension.  That gets flagged as non-portable on
                # Windows, since it contains the drive letter and colon.
                scmutil.checkportable(ui, os.path.join(prefix, f))
                try:
                    st = lstat(f)
                except OSError:
                    if not quiet:
                        ui.warn(_("%s does not exist!\n") % uipath(f))
                    rejected.append(f)
                    continue
                if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
                    if not quiet:
                        ui.warn(
                            _(
                                "%s not added: only files and symlinks "
                                "supported currently\n"
                            )
                            % uipath(f)
                        )
                    rejected.append(f)
                elif ds[f] in "amn":
                    if not quiet:
                        ui.warn(_("%s already tracked!\n") % uipath(f))
                elif ds[f] == "r":
                    ds.normallookup(f)
                else:
                    ds.add(f)
            return rejected

    def forget(self, files, prefix="", quiet=False):
        with self._repo.wlock():
            ds = self._repo.dirstate
            uipath = lambda f: ds.pathto(pathutil.join(prefix, f))
            rejected = []
            for f in files:
                if f not in self._repo.dirstate:
                    if not quiet:
                        self._repo.ui.warn(_("%s not tracked!\n") % uipath(f))
                    rejected.append(f)
                elif self._repo.dirstate[f] != "a":
                    self._repo.dirstate.remove(f)
                else:
                    self._repo.dirstate.untrack(f)
            return rejected

    def undelete(self, list):
        pctxs = self.parents()
        with self._repo.wlock():
            ds = self._repo.dirstate
            for f in list:
                if self._repo.dirstate[f] != "r":
                    self._repo.ui.warn(_("%s not removed!\n") % ds.pathto(f))
                else:
                    fctx = f in pctxs[0] and pctxs[0][f] or pctxs[1][f]
                    t = fctx.data()
                    self._repo.wwrite(f, t, fctx.flags())
                    self._repo.dirstate.normal(f)

    def copy(self, source, dest):
        try:
            st = self._repo.wvfs.lstat(dest)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            self._repo.ui.warn(
                _("%s does not exist!\n") % self._repo.dirstate.pathto(dest)
            )
            return
        if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
            self._repo.ui.warn(
                _("copy failed: %s is not a file or a " "symbolic link\n")
                % self._repo.dirstate.pathto(dest)
            )
        else:
            with self._repo.wlock():
                if self._repo.dirstate[dest] in "?":
                    self._repo.dirstate.add(dest)
                elif self._repo.dirstate[dest] in "r":
                    self._repo.dirstate.normallookup(dest)
                self._repo.dirstate.copy(source, dest)

    def match(
        self,
        pats=None,
        include=None,
        exclude=None,
        default="glob",
        badfn=None,
        emptyalways=True,
        warn=None,
    ):
        r = self._repo

        # Only a case insensitive filesystem needs magic to translate user input
        # to actual case in the filesystem.
        icasefs = not util.fscasesensitive(r.root)
        return matchmod.match(
            r.root,
            r.getcwd(),
            pats,
            include,
            exclude,
            default,
            auditor=r.auditor,
            ctx=self,
            badfn=badfn,
            icasefs=icasefs,
            emptyalways=emptyalways,
            warn=warn,
        )

    def _filtersuspectsymlink(self, files):
        """Filter out changes that make symlinks invalid

        ``unsafe.filtersuspectsymlink`` option allows to enable/disable this
        safety check.
        """
        if (
            not files
            or self._repo.dirstate._checklink
            or not self._repo.ui.configbool("unsafe", "filtersuspectsymlink")
        ):
            return files

        # Symlink placeholders may get non-symlink-like contents
        # via user error or dereferencing by NFS or Samba servers,
        # so we filter out any placeholders that don't look like a
        # symlink
        sane = []
        for f in files:
            if self.flags(f) == "l":
                d = self[f].data()
                if d == b"" or len(d) >= 1024 or b"\n" in d or util.binary(d):
                    self._repo.ui.debug(
                        "ignoring suspect symlink placeholder" ' "%s"\n' % f
                    )
                    continue
            sane.append(f)
        return sane

    def _dirstatestatus(self, match, ignored=False, clean=False, unknown=False):
        """Gets the status from the dirstate -- internal use only."""
        s = self._repo.dirstate.status(
            match, ignored=ignored, clean=clean, unknown=unknown
        )

        # Extend status with submodule status
        if not self.isinmemory() and git.isgitformat(self._repo):
            submodulestatus = git.submodulestatus(self)
            for path, (oldnode, newnode) in submodulestatus.items():
                if not match(path):
                    continue
                if oldnode is None:
                    if newnode is not None:
                        s.added.append(path)
                elif newnode is None:
                    if path not in s.removed:
                        s.removed.append(path)
                elif newnode != oldnode:
                    s.modified.append(path)
            submodulepaths = set(submodulestatus)
            # The directory walk might report submodule paths as "deleted"
            # ("!"). Since we got the real states above, remove the incorrect
            # "!" state.
            if s.deleted and submodulepaths:
                deleted = [p for p in s.deleted if p not in submodulepaths]
                s.deleted[:] = deleted

        if match.always():
            # cache for performance
            if s.unknown or s.ignored or s.clean:
                # "_status" is cached with list*=False in the normal route
                self._status = scmutil.status(
                    s.modified, s.added, s.removed, s.deleted, [], [], []
                )
            else:
                self._status = s

        return s

    @propertycache
    def _manifest(self):
        """generate a manifest corresponding to the values in self._status

        This reuse the file nodeid from parent, but we use special node
        identifiers for added and modified files. This is used by manifests
        merge to see that files are different and by update logic to avoid
        deleting newly added files.
        """
        return self.buildstatusmanifest(self._status)

    def buildstatusmanifest(self, status):
        """Builds a manifest that includes the given status results."""
        parents = self.parents()

        man = parents[0].manifest().copy()

        ff = self._flagfunc
        for f in status.deleted + status.removed:
            if f in man:
                del man[f]

        if not self.isinmemory() and git.isgitformat(self._repo):
            submodulestatus = git.submodulestatus(self)
        else:
            submodulestatus = {}
        for i, l in ((addednodeid, status.added), (modifiednodeid, status.modified)):
            for f in l:
                submodulenodes = submodulestatus.get(f)
                if submodulenodes is not None:
                    oldnode, newnode = submodulenodes
                    # newnode should not be None, but fs is racy...
                    man.set(f, newnode or oldnode, "m")
                    continue
                man[f] = i
                try:
                    man.setflag(f, ff(f))
                except OSError:
                    pass

        return man

    def _buildstatus(self, other, s, match, listignored, listclean, listunknown):
        """build a status with respect to another context

        This includes logic for maintaining the fast path of status when
        comparing the working directory against its parent, which is to skip
        building a new manifest if self (working directory) is not comparing
        against its parent (repo['.']).
        """
        # After calling status below, we compare `other` with the current
        # working copy parent. There's a potential race condition where the
        # working copy parent changes while or after we do the status, and
        # therefore resolving self._repo["."] could result in a different,
        # incorrect parent. Let's grab a copy of it now, so the parent is
        # consistent before and after.
        pctx = self._repo["."]

        s = self._dirstatestatus(match, listignored, listclean, listunknown)
        # Filter out symlinks that, in the case of FAT32 and NTFS filesystems,
        # might have accidentally ended up with the entire contents of the file
        # they are supposed to be linking to.
        s.modified[:] = self._filtersuspectsymlink(s.modified)
        if other != pctx:
            s = super(workingctx, self)._buildstatus(
                other, s, match, listignored, listclean, listunknown
            )
        return s

    def _matchstatus(self, other, match):
        """override the match method with a filter for directory patterns

        We use inheritance to customize the match.bad method only in cases of
        workingctx since it belongs only to the working directory when
        comparing against the parent changeset.

        If we aren't comparing against the working directory's parent, then we
        just use the default match object sent to us.
        """
        if other != self._repo["."]:
            origbad = match.bad

            def bad(f, msg):
                # 'f' may be a directory pattern from 'match.files()',
                # so 'f not in ctx1' is not enough
                if f not in other and not other.hasdir(f):
                    origbad(f, msg)

            match.bad = bad
        return match

    def markcommitted(self, node):
        super(workingctx, self).markcommitted(node)


class committablefilectx(basefilectx):
    """A committablefilectx provides common functionality for a file context
    that wants the ability to commit, e.g. workingfilectx or memfilectx."""

    def __init__(self, repo, path, filelog=None, ctx=None):
        self._repo = repo
        self._path = path
        self._changeid = None
        self._filerev = self._filenode = None

        if filelog is not None:
            self._filelog = filelog
        if ctx:
            self._changectx = ctx

    def __nonzero__(self):
        return True

    __bool__ = __nonzero__

    def linkrev(self):
        # linked to self._changectx no matter if file is modified or not
        return self.rev()

    def parents(self):
        """return parent filectxs, following copies if necessary"""

        def filenode(ctx, path):
            return ctx._manifest.get(path, nullid)

        path = self._path
        fl = self._filelog
        pcl = self._changectx._parents
        renamed = self.renamed()

        if renamed:
            pl = [renamed + (None,)]
        else:
            pl = [(path, filenode(pcl[0], path), fl)]

        for pc in pcl[1:]:
            pl.append((path, filenode(pc, path), fl))

        return [
            self._parentfilectx(p, fileid=n, filelog=l) for p, n, l in pl if n != nullid
        ]

    def children(self):
        return []


class workingfilectx(committablefilectx):
    """A workingfilectx object makes access to data related to a particular
    file in the working directory convenient."""

    def __init__(self, repo, path, filelog=None, workingctx=None):
        super(workingfilectx, self).__init__(repo, path, filelog, workingctx)

    @propertycache
    def _changectx(self):
        return workingctx(self._repo)

    def filenode(self):
        # Usually filenode is None until repo.commitctx, but submodule is an
        # exception. Resolve submodule node so repo.commitctx can use it.
        if self._filenode is None and self.flags() == "m":
            self._filenode = self.changectx().manifest().get(self.path())
        return self._filenode

    def data(self):
        try:
            return self._repo.wread(self._path)
        except IOError as e:
            if e.errno == errno.EISDIR:
                # might be a submodule
                if self.flags() == "m":
                    text = "Subproject commit %s\n" % hex(self.filenode())
                    return text.encode("utf-8")
            raise

    def content_sha256(self):
        return hashlib.sha256(self.data()).digest()

    def renamed(self):
        rp = self._repo.dirstate.copied(self._path)
        if not rp:
            return None
        return rp, self._changectx._parents[0]._manifest.get(rp, nullid)

    def size(self):
        return self._repo.wvfs.lstat(self._path).st_size

    def date(self):
        t, tz = self._changectx.date()
        try:
            return (self._repo.wvfs.lstat(self._path).st_mtime, tz)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            return (t, tz)

    def exists(self):
        return self._repo.wvfs.exists(self._path)

    def lexists(self):
        return self._repo.wvfs.lexists(self._path)

    def audit(self):
        return self._repo.wvfs.audit(self._path)

    def workingflags(self):
        """Returns this file's flags ('l', 'x', 'lx', or '') by inspecting
        the working copy.

        This *should* be the default behavior of flags() to mimic data(),
        size(), exists(), etc.; since it does not exist, flags() returns the
        p1's flags for ``file`` instead.

        However, too much existing code relies on this to switch whole hog.
        """
        vfs = self._repo.wvfs
        flags = [vfs.islink(self._path) and "l", vfs.isexec(self._path) and "x"]
        return "".join(filter(None, flags))

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        if isinstance(fctx, workingfilectx):
            if self.size() != fctx.size():
                return True
            if self.flags() != fctx.flags():
                return True
            if self._repo.ui.configbool("scmstore", "status"):
                return self.content_sha256() != fctx.content_sha256()
            if self.data() != fctx.data():
                return True
            return False

        # fctx is not a workingfilectx
        # invert comparison to reuse the same code path
        return fctx.cmp(self)

    def remove(self, ignoremissing=False):
        """wraps unlink for a repo's working directory"""
        self._repo.wvfs.unlinkpath(self._path, ignoremissing=ignoremissing)

    def write(self, data, flags, backgroundclose=False):
        """wraps repo.wwrite"""
        self._repo.wwrite(self._path, data, flags, backgroundclose=backgroundclose)

    def markcopied(self, src):
        """marks this file a copy of `src`"""
        if self._repo.dirstate[self._path] in "nma":
            self._repo.dirstate.copy(src, self._path)

    def clearunknown(self):
        """Removes conflicting items in the working directory so that
        ``write()`` can be called successfully.
        """
        wvfs = self._repo.wvfs
        f = self._path
        wvfs.audit(f)
        if wvfs.isdir(f) and not wvfs.islink(f):
            wvfs.rmtree(f, forcibly=True)
        for p in reversed(list(util.finddirs(f))):
            if wvfs.isfileorlink(p):
                wvfs.unlink(p)
                break

    def setflags(self, l, x):
        self._repo.wvfs.setflags(self._path, l, x)


class overlayworkingctx(committablectx):
    """Wraps another mutable context with a write-back cache that can be
    converted into a commit context.

    self._cache[path] maps to a dict with keys: {
        'exists': bool?
        'date': date?
        'data': str?
        'flags': str?
        'copied': str? (path or None)
    }
    If `exists` is True, `flags` must be non-None and 'date' is non-None. If it
    is `False`, the file was deleted.
    """

    def __init__(self, repo):
        super(overlayworkingctx, self).__init__(repo)
        self._repo = repo
        self.clean()

    def setbase(self, wrappedctx):
        self._wrappedctx = wrappedctx
        self._parents = [wrappedctx]
        # Drop old manifest cache as it is now out of date.
        # This is necessary when, e.g., rebasing several nodes with one
        # ``overlayworkingctx`` (e.g. with --collapse).
        util.clearcachedproperty(self, "_manifest")

    def data(self, path):
        if self.isdirty(path):
            if self._cache[path]["exists"]:
                if self._cache[path]["data"] is not None:
                    return self._cache[path]["data"]
                else:
                    # Must fallback here, too, because we only set flags.
                    return self._wrappedctx[path].data()
            else:
                raise error.ProgrammingError("No such file or directory: %s" % path)
        else:
            return self._wrappedctx[path].data()

    @propertycache
    def _manifest(self):
        parents = self.parents()
        man = parents[0].manifest().copy()

        flag = self._flagfunc
        for path in self.added():
            man[path] = addednodeid
            man.setflag(path, flag(path))
        for path in self.modified():
            man[path] = modifiednodeid
            man.setflag(path, flag(path))
        for path in self.removed():
            del man[path]
        return man

    @propertycache
    def _flagfunc(self):
        def f(path):
            return self._cache[path]["flags"]

        return f

    def files(self):
        return sorted(self.added() + self.modified() + self.removed())

    def modified(self):
        return [
            f
            for f in self._cache.keys()
            if self._cache[f]["exists"] and self._existsinparent(f)
        ]

    def added(self):
        return [
            f
            for f in self._cache.keys()
            if self._cache[f]["exists"] and not self._existsinparent(f)
        ]

    def removed(self):
        return [
            f
            for f in self._cache.keys()
            if not self._cache[f]["exists"] and self._existsinparent(f)
        ]

    def isinmemory(self):
        return True

    def filedate(self, path):
        if self.isdirty(path):
            return self._cache[path]["date"]
        else:
            return self._wrappedctx[path].date()

    def markcopied(self, path, origin):
        if self.isdirty(path):
            self._cache[path]["copied"] = origin
        else:
            if self.exists(path):
                self._markdirty(
                    path,
                    True,
                    data=self.data(path),
                    flags=self.flags(path),
                    copied=origin,
                )
            else:
                raise error.ProgrammingError("markcopied() called on non-existent file")

    def copydata(self, path):
        if self.isdirty(path):
            return self._cache[path]["copied"]
        else:
            raise error.ProgrammingError("copydata() called on clean context")

    def flags(self, path):
        if self.isdirty(path):
            if self._cache[path]["exists"]:
                return self._cache[path]["flags"]
            else:
                raise error.ProgrammingError(
                    "No such file or directory: %s" % self._path
                )
        else:
            return self._wrappedctx[path].flags()

    def _existsinparent(self, path):
        try:
            # ``commitctx` raises a ``ManifestLookupError`` if a path does not
            # exist, unlike ``workingctx``, which returns a ``workingfilectx``
            # with an ``exists()`` function.
            self._wrappedctx[path]
            return True
        except error.ManifestLookupError:
            return False

    def write(self, path, data, flags=""):
        if data is None:
            raise error.ProgrammingError("data must be non-None")
        self._markdirty(path, exists=True, data=data, date=util.makedate(), flags=flags)

    def setflags(self, path, l, x):
        self._markdirty(
            path,
            exists=True,
            date=util.makedate(),
            flags=(l and "l" or "") + (x and "x" or ""),
        )

    def remove(self, path):
        self._markdirty(path, exists=False)

    def exists(self, path):
        """exists behaves like `lexists`, but needs to follow symlinks and
        return False if they are broken.
        """
        if self.isdirty(path):
            # If this path exists and is a symlink, "follow" it by calling
            # exists on the destination path.
            if self._cache[path]["exists"] and "l" in self._cache[path]["flags"]:
                return self.exists(self._cache[path]["data"].strip())
            else:
                return self._cache[path]["exists"]

        return self._existsinparent(path)

    def __contains__(self, path):
        return self.exists(path)

    def lexists(self, path):
        """lexists returns True if the path exists"""
        if self.isdirty(path):
            return self._cache[path]["exists"]

        return self._existsinparent(path)

    def size(self, path):
        if self.isdirty(path):
            if self._cache[path]["exists"]:
                return len(self._cache[path]["data"])
            else:
                raise error.ProgrammingError(
                    "No such file or directory: %s" % self._path
                )
        return self._wrappedctx[path].size()

    def tomemctx(
        self,
        text,
        branch=None,
        extra=None,
        date=None,
        parents=None,
        user=None,
        editor=None,
        loginfo=None,
        mutinfo=None,
    ):
        """Converts this ``overlayworkingctx`` into a ``memctx`` ready to be
        committed.

        ``text`` is the commit message.
        ``parents`` (optional) are rev numbers.
        """
        # Default parents to the wrapped contexts' if not passed.
        if parents is None:
            parents = self._wrappedctx.parents()
            if len(parents) == 1:
                parents = (parents[0], None)

        # ``parents`` is passed as rev numbers; convert to ``commitctxs``.
        if parents[1] is None:
            parents = (self._repo[parents[0]], None)
        else:
            parents = (self._repo[parents[0]], self._repo[parents[1]])

        files = self._cache.keys()

        def getfile(repo, memctx, path):
            if self._cache[path]["exists"]:
                return memfilectx(
                    repo,
                    memctx,
                    path,
                    self.data(path),
                    copied=self._cache[path]["copied"],
                    flags=self.flags(path),
                )
            else:
                # Returning None, but including the path in `files`, is
                # necessary for memctx to register a deletion.
                return None

        return memctx(
            self._repo,
            parents,
            text,
            files,
            getfile,
            date=date,
            extra=extra,
            user=user,
            branch=branch,
            editor=editor,
            loginfo=loginfo,
            mutinfo=mutinfo,
        )

    def isdirty(self, path):
        return path in self._cache

    def isempty(self):
        # We need to discard any keys that are actually clean before the empty
        # commit check.
        self._compact()
        return len(self._cache) == 0

    def clean(self):
        self._cache = {}

    def _compact(self):
        """Removes keys from the cache that are actually clean, by comparing
        them with the underlying context.

        This can occur during the merge process, e.g. by passing --tool :local
        to resolve a conflict.
        """
        keys = []
        for path in self._cache.keys():
            cache = self._cache[path]
            try:
                underlying = self._wrappedctx[path]
                if (
                    underlying.data() == cache["data"]
                    and underlying.flags() == cache["flags"]
                ):
                    keys.append(path)
            except error.ManifestLookupError:
                # Path not in the underlying manifest (created).
                continue

        for path in keys:
            del self._cache[path]
        return keys

    def _markdirty(self, path, exists, data=None, date=None, flags="", copied=None):
        self._cache[path] = {
            "exists": exists,
            "data": data,
            "date": date,
            "flags": flags,
            "copied": copied,
        }

    def filectx(self, path, filelog=None):
        return overlayworkingfilectx(self._repo, path, parent=self, filelog=filelog)


class overlayworkingfilectx(committablefilectx):
    """Wrap a ``workingfilectx`` but intercepts all writes into an in-memory
    cache, which can be flushed through later by calling ``flush()``."""

    def __init__(self, repo, path, filelog=None, parent=None):
        super(overlayworkingfilectx, self).__init__(repo, path, filelog, parent)
        self._repo = repo
        self._parent = parent
        self._path = path

    def cmp(self, fctx):
        if self._repo.ui.configbool("scmstore", "status"):
            return self.content_sha256() != fctx.content_sha256()
        return self.data() != fctx.data()

    def changectx(self):
        return self._parent

    def data(self):
        return self._parent.data(self._path)

    def date(self):
        return self._parent.filedate(self._path)

    def exists(self):
        return self.lexists()

    def lexists(self):
        return self._parent.exists(self._path)

    def renamed(self):
        path = self._parent.copydata(self._path)
        if not path:
            return None
        return path, self._changectx._parents[0]._manifest.get(path, nullid)

    def size(self):
        return self._parent.size(self._path)

    def markcopied(self, origin):
        self._parent.markcopied(self._path, origin)

    def audit(self):
        pass

    def flags(self):
        return self._parent.flags(self._path)

    def setflags(self, islink, isexec):
        return self._parent.setflags(self._path, islink, isexec)

    def write(self, data, flags, backgroundclose=False):
        return self._parent.write(self._path, data, flags)

    def remove(self, ignoremissing=False):
        return self._parent.remove(self._path)

    def clearunknown(self):
        pass


class workingcommitctx(workingctx):
    """A workingcommitctx object makes access to data related to
    the revision being committed convenient.

    This hides changes in the working directory, if they aren't
    committed in this context.
    """

    def __init__(
        self,
        repo,
        changes,
        text="",
        user=None,
        date=None,
        extra=None,
        loginfo=None,
        mutinfo=None,
    ):
        super(workingctx, self).__init__(
            repo, text, user, date, extra, changes, loginfo, mutinfo
        )

    def _dirstatestatus(self, match, ignored=False, clean=False, unknown=False):
        """Return matched files only in ``self._status``

        Uncommitted files appear "clean" via this context, even if
        they aren't actually so in the working directory.
        """
        if clean:
            clean = [f for f in self._manifest if f not in self._changedset]
        else:
            clean = []
        return scmutil.status(
            [f for f in self._status.modified if match(f)],
            [f for f in self._status.added if match(f)],
            [f for f in self._status.removed if match(f)],
            [],
            [],
            [],
            clean,
        )

    @propertycache
    def _changedset(self):
        """Return the set of files changed in this context"""
        changed = set(self._status.modified)
        changed.update(self._status.added)
        changed.update(self._status.removed)
        return changed


def makecachingfilectxfn(func):
    """Create a filectxfn that caches based on the path.

    We can't use util.cachefunc because it uses all arguments as the cache
    key and this creates a cycle since the arguments include the repo and
    memctx.
    """
    cache = {}

    def getfilectx(repo, memctx, path):
        if path not in cache:
            cache[path] = func(repo, memctx, path)
        return cache[path]

    return getfilectx


def memfilefromctx(ctx):
    """Given a context return a memfilectx for ctx[path]

    This is a convenience method for building a memctx based on another
    context.
    """

    def getfilectx(repo, memctx, path):
        fctx = ctx[path]
        # this is weird but apparently we only keep track of one parent
        # (why not only store that instead of a tuple?)
        copied = fctx.renamed()
        if copied:
            copied = copied[0]
        return memfilectx(
            repo,
            memctx,
            path,
            fctx.data(),
            islink=fctx.islink(),
            isexec=fctx.isexec(),
            copied=copied,
        )

    return getfilectx


def memfilefrompatch(patchstore):
    """Given a patch (e.g. patchstore object) return a memfilectx

    This is a convenience method for building a memctx based on a patchstore.
    """

    def getfilectx(repo, memctx, path):
        data, mode, copied = patchstore.getfile(path)
        if data is None:
            return None
        islink, isexec = mode
        return memfilectx(
            repo, memctx, path, data, islink=islink, isexec=isexec, copied=copied
        )

    return getfilectx


class memctx(committablectx):
    """Use memctx to perform in-memory commits via localrepo.commitctx().

    Revision information is supplied at initialization time while
    related files data and is made available through a callback
    mechanism.  'repo' is the current localrepo, 'parents' is a
    sequence of two parent revisions identifiers (pass None for every
    missing parent), 'text' is the commit message and 'files' lists
    names of files touched by the revision (normalized and relative to
    repository root).

    filectxfn(repo, memctx, path) is a callable receiving the
    repository, the current memctx object and the normalized path of
    requested file, relative to repository root. It is fired by the
    commit function for every file in 'files', but calls order is
    undefined. If the file is available in the revision being
    committed (updated or added), filectxfn returns a memfilectx
    object. If the file was removed, filectxfn return None for recent
    Mercurial. Moved files are represented by marking the source file
    removed and the new file added with copy information (see
    memfilectx).

    user receives the committer name and defaults to current
    repository username, date is the commit date in any format
    supported by util.parsedate() and defaults to current date, extra
    is a dictionary of metadata or is left empty.
    """

    # Mercurial <= 3.1 expects the filectxfn to raise IOError for missing files.
    # Extensions that need to retain compatibility across Mercurial 3.1 can use
    # this field to determine what to do in filectxfn.
    _returnnoneformissingfiles = True

    def __init__(
        self,
        repo,
        parents,
        text,
        files,
        filectxfn,
        user=None,
        date=None,
        extra=None,
        branch=None,
        editor=False,
        loginfo=None,
        mutinfo=None,
    ):
        super(memctx, self).__init__(
            repo, text, user, date, extra, loginfo=loginfo, mutinfo=mutinfo
        )
        self._node = None
        # The api for parents has changed over time. Let's normalize the inputs
        # by filtering out nullid and None entries.
        parents = [p for p in parents if p is not None and p.node() != nullid]
        if len(parents) == 0:
            parents = [repo[nullid]]
        self._parents = parents
        self._filesset = set(files)
        if branch is not None:
            self._extra["branch"] = encoding.fromlocal(branch)

        if isinstance(filectxfn, patch.filestore):
            filectxfn = memfilefrompatch(filectxfn)
        elif not callable(filectxfn):
            # if store is not callable, wrap it in a function
            filectxfn = memfilefromctx(filectxfn)

        # memoizing increases performance for e.g. vcs convert scenarios.
        self._filectxfn = makecachingfilectxfn(filectxfn)

        # used by __setitem__
        self._fctxoverrides = {}

        if editor:
            self._text = editor(self._repo, self)
            self._repo.savecommitmessage(self._text)

    @classmethod
    def mirror(
        cls,
        ctx,
        user=None,
        date=None,
        text=None,
        extra=None,
        parents=None,
        mutinfo=None,
        loginfo=None,
        editor=False,
    ):
        """mirror another ctx, make it "mutable"

        Note: This is different from overlayworkingctx in 2 ways:

        1. memctx does not deep copy file contexts. So if the ctx has LFS
           files that are lazily loaded, those files are still lazily loaded
           with the new memctx.

        2. memctx.mirror is for "amend", while overlayworkingctx.setbase is for
           "commit on top of". memctx.mirror can also be used for "commit", if
           passing in workingctx or another memctx. overlayworkingctx would
           require deep copying for the "amend" use-case.
        """
        repo = ctx.repo()
        if parents is None:
            parents = ctx.parents()
        if text is None:
            text = ctx.description()
        if user is None:
            user = ctx.user()
        if date is None:
            date = ctx.date()
        if extra is None:
            extra = ctx.extra()
        if mutinfo is None:
            mutinfo = getattr(ctx, "mutinfo", lambda: None)()
        if loginfo is None:
            loginfo = getattr(ctx, "loginfo", lambda: None)()

        def filectxfn(_repo, _ctx, path, ctx=ctx, parents=parents):
            # If the path is specifically manipulated by this commit, load this
            # commit's version. Otherwise load the parents version. Note,
            # self._parents[0] may be different from ctx.p1(), which is why we
            # have to reference it specifically instead of going through ctx.
            if path in ctx.files():
                ctx = ctx
            elif len(parents) > 0:
                ctx = parents[0]
                if len(parents) > 1 and path not in ctx and path in parents[1]:
                    ctx = parents[1]
            else:
                return None

            if path in ctx:
                return ctx[path]
            else:
                # deleted file
                return None

        mctx = cls(
            repo,
            parents=parents,
            text=text,
            files=ctx.files(),
            filectxfn=filectxfn,
            user=user,
            date=date,
            extra=extra,
            loginfo=loginfo,
            mutinfo=mutinfo,
        )

        if editor:
            mctx._text = editor(repo, mctx)
            repo.savecommitmessage(mctx._text)

        return mctx

    def filectx(self, path, filelog=None):
        """get a file context from the working directory

        Returns None if file doesn't exist and should be removed."""
        if path in self._fctxoverrides:
            return self._fctxoverrides[path]
        return self._filectxfn(self._repo, self, path)

    def commit(self):
        """commit context to the repo"""
        return self._repo.commitctx(self)

    @propertycache
    def _manifest(self):
        """generate a manifest based on the return values of filectxfn"""

        # keep this simple for now; just worry about p1
        pctx = self._parents[0]
        man = pctx.manifest().copy()

        for f in self._status.modified:
            if git.isgitformat(self._repo):
                man[f] = git.hashobj(b"blob", self[f].data())
            else:
                p1node = nullid
                p2node = nullid
                p = pctx[f].parents()  # if file isn't in pctx, check p2?
                if len(p) > 0:
                    p1node = p[0].filenode()
                    if len(p) > 1:
                        p2node = p[1].filenode()
                man[f] = revlog.hash(self[f].data(), p1node, p2node)

        for f in self._status.added:
            if git.isgitformat(self._repo):
                man[f] = git.hashobj(b"blob", self[f].data())
            else:
                man[f] = revlog.hash(self[f].data(), nullid, nullid)

        for f in self._status.removed:
            if f in man:
                del man[f]

        return man

    @propertycache
    def _status(self):
        """Calculate exact status from ``files`` specified at construction"""
        man1 = self.p1().manifest()
        if len(self._parents) == 2:
            man2 = self._parents[1].manifest()
            managing = lambda f: f in man1 or f in man2
        else:
            managing = lambda f: f in man1

        modified, added, removed = [], [], []
        for f in sorted(self._filesset):
            inparents = managing(f)
            inself = self[f] is not None
            if inparents:
                if inself:
                    modified.append(f)
                else:
                    removed.append(f)
            else:
                if inself:
                    added.append(f)

        return scmutil.status(modified, added, removed, [], [], [], [])

    def __setitem__(self, path, fctx):
        """Set a path to the given fctx.

        This invalidates caches of `status` and `manifest`. For performance,
        avoid mixing `__setitem__` and read operation like `__contains__`,
        `added`, etc in loops.
        """
        # This API is intended to be used in "dirsync" cases. For example, the
        # following code copies the file from srcpath to dstpath in-memory:
        #
        #    ctx[dstpath] = ctx[srcpath]
        #
        # Note: There are no checks for all fields in `fctx`. For
        # `repo.commitctx` to work correctly, the `path`, `data`, `renamed` of
        # `fctx` need to be correct, and other fields like `p1`, `p2`, `ctx` do
        # not really matter.
        #
        # This function intentionally avoids updating the `ctx` field to `self`
        # so LFS fast paths can still work.
        self._filesset.add(path)

        # invalidate cache
        self.__dict__.pop("_status", None)
        self.__dict__.pop("_manifest", None)

        if fctx is not None and fctx.path() != path:
            # fix "path" automatically.
            fctx = overlayfilectx(fctx, path=path)
        self._fctxoverrides[path] = fctx


class memfilectx(committablefilectx):
    """memfilectx represents an in-memory file to commit.

    See memctx and committablefilectx for more details.
    """

    def __init__(
        self,
        repo,
        changectx,
        path,
        data,
        islink=False,
        isexec=False,
        copied=None,
        flags=None,
    ):
        """
        path is the normalized file path relative to repository root.
        data is the file content as a string.
        islink is True if the file is a symbolic link (for compatibility).
        isexec is True if the file is executable (for compatibility).
        flags provides the "flags" directly, bypassing islink and isexec.
        copied is the source file path if current file was copied in the
        revision being committed, or None."""
        super(memfilectx, self).__init__(repo, path, None, changectx)
        self._data = data
        if flags is None:
            self._flags = (islink and "l" or "") + (isexec and "x" or "")
        else:
            self._flags = flags
        self._copied = None
        if self._flags == "m":
            # HACK: Reconstruct filenode from "data()" for submodules.
            # Ideally the callsite provides filenode, but that's not a
            # trivial change.
            self._filenode = bin(data.rstrip()[-len(nullhex) :])
        if copied:
            self._copied = (copied, nullid)

    def data(self):
        return self._data

    def remove(self, ignoremissing=False):
        """wraps unlink for a repo's working directory"""
        # need to figure out what to do here
        del self._changectx[self._path]

    def write(self, data, flags):
        """wraps repo.wwrite"""
        self._data = data


class overlayfilectx(committablefilectx):
    """Like memfilectx but take an original filectx and optional parameters to
    override parts of it. This is useful when fctx.data() is expensive (i.e.
    flag processor is expensive) and raw data, flags, and filenode could be
    reused (ex. rebase or mode-only amend a REVIDX_EXTSTORED file).
    """

    def __init__(
        self, originalfctx, datafunc=None, path=None, flags=None, copied=None, ctx=None
    ):
        """originalfctx: filecontext to duplicate

        datafunc: None or a function to override data (file content). It is a
        function to be lazy. path, flags, copied, ctx: None or overridden value

        copied could be (path, rev), or False. copied could also be just path,
        and will be converted to (path, nullid). This simplifies some callers.
        """

        if path is None:
            path = originalfctx.path()
        if ctx is None:
            ctx = originalfctx.changectx()
            ctxmatch = lambda: True
        else:

            def ctxmatch():
                if ctx == originalfctx.changectx():
                    return True
                if ctx.node() is None:
                    # memory context
                    return True
                return False

        repo = originalfctx.repo()
        flog = originalfctx.filelog()
        super(overlayfilectx, self).__init__(repo, path, flog, ctx)

        if copied is None:
            copied = originalfctx.renamed()
            copiedmatch = lambda: True
        else:
            if copied and not isinstance(copied, tuple):
                # repo._filecommit will recalculate copyrev so nullid is okay
                copied = (copied, nullid)
            copiedmatch = lambda: copied == originalfctx.renamed()

        # When data, copied (could affect data), ctx (could affect filelog
        # parents) are not overridden, rawdata, rawflags, and filenode may be
        # reused (repo._filecommit should double check filelog parents).
        #
        # path, flags are not hashed in filelog (but in manifestlog) so they do
        # not affect reusable here.
        #
        # If ctx or copied is overridden to a same value with originalfctx,
        # still consider it's reusable. originalfctx.renamed() may be a bit
        # expensive so it's not called unless necessary. Assuming datafunc is
        # always expensive, do not call it for this "reusable" test.
        reusable = datafunc is None and ctxmatch() and copiedmatch()

        if datafunc is None:
            datafunc = originalfctx.data
        if flags is None:
            flags = originalfctx.flags()

        self._datafunc = datafunc
        self._flags = flags
        self._copied = copied

        if reusable:
            # copy extra fields from originalfctx
            attrs = ["rawdata", "rawflags", "_filenode", "_filerev"]
            for attr_ in attrs:
                if util.safehasattr(originalfctx, attr_):
                    setattr(self, attr_, getattr(originalfctx, attr_))

    def data(self):
        return self._datafunc()


class metadataonlyctx(committablectx):
    """Like memctx but it's reusing the manifest of different commit.
    Intended to be used by lightweight operations that are creating
    metadata-only changes.

    Revision information is supplied at initialization time.  'repo' is the
    current localrepo, 'ctx' is original revision which manifest we're reuisng
    'parents' is a sequence of two parent revisions identifiers (pass None for
    every missing parent), 'text' is the commit.

    user receives the committer name and defaults to current repository
    username, date is the commit date in any format supported by
    util.parsedate() and defaults to current date, extra is a dictionary of
    metadata or is left empty.
    """

    def __new__(cls, repo, originalctx, *args, **kwargs):
        return super(metadataonlyctx, cls).__new__(cls, repo)

    def __init__(
        self,
        repo,
        originalctx,
        parents=None,
        text=None,
        user=None,
        date=None,
        extra=None,
        editor=False,
        loginfo=None,
        mutinfo=None,
    ):
        if text is None:
            text = originalctx.description()
        super(metadataonlyctx, self).__init__(
            repo, text, user, date, extra, loginfo=loginfo, mutinfo=mutinfo
        )
        self._node = None
        self._originalctx = originalctx
        self._manifestnode = originalctx.manifestnode()
        if parents is None:
            parents = originalctx.parents()
        else:
            parents = [repo[p] for p in parents if p is not None]
        self._parents = [p for p in parents if p.node() != nullid]
        parents = parents[:]
        while len(parents) < 2:
            parents.append(repo[nullid])
        p1, p2 = parents

        # sanity check to ensure that our parent's manifest has not changed
        # from our original parent's manifest to ensure the caller is not
        # creating invalid commits
        ops = [repo[p] for p in originalctx.parents() if p is not None]
        while len(ops) < 2:
            ops.append(repo[nullid])
        op1, op2 = ops

        if p1.manifestnode() != op1.manifestnode():
            raise RuntimeError(
                "new p1 manifest (%s) is not the old p1 manifest (%s)"
                % (hex(p1.manifestnode()), hex(op1.manifestnode()))
            )
        if p2.manifestnode() != op2.manifestnode():
            raise RuntimeError(
                "new p2 manifest (%s) is not the old p2 manifest (%s)"
                % (hex(p2.manifestnode()), hex(op2.manifestnode()))
            )

        self._files = originalctx.files()

        if editor:
            self._text = editor(self._repo, self)
            self._repo.savecommitmessage(self._text)

    def manifestnode(self):
        return self._manifestnode

    @property
    def _manifestctx(self):
        return self._repo.manifestlog[self._manifestnode]

    def filectx(self, path, filelog=None):
        return self._originalctx.filectx(path, filelog=filelog)

    def commit(self):
        """commit context to the repo"""
        return self._repo.commitctx(self)

    @property
    def _manifest(self):
        return self._originalctx.manifest()

    @propertycache
    def _status(self):
        """Calculate exact status from ``files`` specified in the ``origctx``
        and parents manifests.
        """
        man1 = self.p1().manifest()
        if len(self._parents) > 1:
            p2 = self._parents[1]
            man2 = p2.manifest()
            managing = lambda f: f in man1 or f in man2
        else:
            managing = lambda f: f in man1

        modified, added, removed = [], [], []
        for f in self._files:
            if not managing(f):
                added.append(f)
            elif f in self:
                modified.append(f)
            else:
                removed.append(f)

        return scmutil.status(modified, added, removed, [], [], [], [])


class arbitraryfilectx(object):
    """Allows you to use filectx-like functions on a file in an arbitrary
    location on disk, possibly not in the working directory.
    """

    def __init__(self, path, repo=None):
        # Repo is optional because contrib/simplemerge uses this class.
        self._repo = repo
        self._path = path

    def cmp(self, fctx):
        # filecmp follows symlinks whereas `cmp` should not, so skip the fast
        # path if either side is a symlink.
        symlinks = "l" in self.flags() or "l" in fctx.flags()
        if not symlinks and isinstance(fctx, workingfilectx) and self._repo:
            # Add a fast-path for merge if both sides are disk-backed.
            # Note that filecmp uses the opposite return values (True if same)
            # from our cmp functions (True if different).
            return not filecmp.cmp(self.path(), self._repo.wjoin(fctx.path()))
        if self._repo.ui.configbool("scmstore", "status"):
            return self.content_sha256() != fctx.content_sha256()
        return self.data() != fctx.data()

    def path(self):
        return self._path

    def flags(self):
        return ""

    def data(self):
        return util.readfile(self._path)

    def content_sha256(self):
        return hashlib.sha256(self.data()).digest()

    def decodeddata(self):
        with open(self._path, "rb") as f:
            return f.read()

    def remove(self):
        util.unlink(self._path)

    def write(self, data, flags):
        assert not flags
        with open(self._path, "w") as f:
            f.write(data)
