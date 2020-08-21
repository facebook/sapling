# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from typing import IO, Optional, Union

import bindings

from . import (
    bookmarks as bookmod,
    changelog as changelogmod,
    encoding,
    error,
    mdiff,
    progress,
    revlog,
    util,
    visibility,
)
from .changelog import changelogrevision, hgcommittext, readfiles
from .i18n import _
from .node import hex, nullid, nullrev, wdirid, wdirrev
from .pycompat import encodeutf8


SEGMENTS_DIR = "segments/v1"
HGCOMMITS_DIR = "hgcommits/v1"


class changelog(object):
    """Changelog backed by Rust objects.

    Many methods exist for compatibility. New code should consider using `dag`,
    `dageval`, `changelogrevision` for reads, avoiding other read operations.
    """

    _delayed = True
    indexfile = "00changelog.i"
    storedeltachains = False

    def __init__(self, svfs, inner, uiconfig):
        """Construct changelog backed by Rust objects."""
        self.svfs = svfs
        self.inner = inner
        # TODO: Consider moving visibleheads out?
        self._visibleheads = self._loadvisibleheads(svfs)
        self._uiconfig = uiconfig

    def userust(self, name=None):
        return True

    @property
    def opener(self):
        return self.svfs

    @classmethod
    def openrevlog(cls, svfs, uiconfig):
        """Construct changelog from 00changelog.i revlog."""
        inner = bindings.dag.commits.openrevlog(svfs.join(""))
        return cls(svfs, inner, uiconfig)

    @classmethod
    def opensegments(cls, svfs, uiconfig):
        segmentsdir = svfs.join(SEGMENTS_DIR)
        hgcommitsdir = svfs.join(HGCOMMITS_DIR)
        inner = bindings.dag.commits.opensegments(segmentsdir, hgcommitsdir)
        return cls(svfs, inner, uiconfig)

    @classmethod
    def opendoublewrite(cls, svfs, uiconfig):
        revlogdir = svfs.join("")
        segmentsdir = svfs.join(SEGMENTS_DIR)
        hgcommitsdir = svfs.join(HGCOMMITS_DIR)
        inner = bindings.dag.commits.opendoublewrite(
            revlogdir, segmentsdir, hgcommitsdir
        )
        return cls(svfs, inner, uiconfig)

    @property
    def dag(self):
        """Get the DAG with algorithms. Require rust-commit."""
        return self.inner.dagalgo()

    def dageval(self, func, extraenv=None):
        """Evaluate func with the current DAG context.

        Example:

            cl.dageval(lambda: roots(ancestors([x])))

            # avoid conflict with local variable named 'only'
            cl.dageval(lambda dag: dag.only(x, y))

        is equivalent to:

            dag = cl.dag
            dag.roots(dag.ancestors([x]))

        'extraenv' is optionally a dict. It sets extra names. For example,
        providing revset-like functions like `draft`, `public`, `bookmarks`.
        """
        env = dict(func.__globals__)
        dag = self.dag
        for name in func.__code__.co_names:
            # name is potentially a string used by LOAD_GLOBAL bytecode.
            if extraenv is not None and name in extraenv:
                # Provided by 'extraenv'.
                env[name] = extraenv[name]
            else:
                # Provided by 'dag'.
                value = getattr(dag, name, None)
                if value is not None:
                    env[name] = value
        code = func.__code__
        argdefs = func.__defaults__
        closure = func.__closure__
        name = func.__name__
        # Create a new function that uses the same logic except for different
        # env (globals).
        newfunc = type(func)(code, env, name, argdefs, closure)
        if code.co_argcount == 1 and not argdefs:
            return newfunc(dag)
        else:
            return newfunc()

    @property
    def idmap(self):
        """Get the IdMap. Require rust-commit."""
        return self.inner.idmap()

    @property
    def hasnode(self):
        return self.idmap.__contains__

    @property
    def torevs(self):
        """Convert a Set using commit hashes to an IdSet using numbers

        The Set is usually obtained via `self.dag` APIs.
        """
        return self.inner.torevs

    @property
    def tonodes(self):
        """Convert an IdSet to Set. The reverse of torevs."""
        return self.inner.tonodes

    def _loadvisibleheads(self, svfs):
        return visibility.visibleheads(svfs)

    def tip(self):
        # type: () -> bytes
        return self.dag.all().first() or nullid

    def __contains__(self, rev):
        """filtered version of revlog.__contains__"""
        return rev is not None and rev in self.torevs(self.dag.all())

    def __iter__(self):
        """filtered version of revlog.__iter__"""
        return self.torevs(self.dag.all()).iterasc()

    def __len__(self):
        return len(self.dag.all())

    def revs(self, start=0, stop=None):
        """filtered version of revlog.revs"""
        allrevs = self.torevs(self.dag.all())
        if stop is not None:
            # exclusive -> inclusive
            stop = stop - 1
        revs = bindings.dag.spans.unsaferange(start, stop) & allrevs
        for i in revs.iterasc():
            yield i

    @property
    def nodemap(self):
        # self.idmap might change, do not cache this nodemap result
        return nodemap(self)

    def reachableroots(self, minroot, heads, roots, includepath=False):
        tonodes = self.tonodes
        headnodes = tonodes(heads)
        rootnodes = tonodes(roots)
        dag = self.dag
        if includepath:
            # special case: null::X -> ::X
            if len(rootnodes) == 0 and nullrev in roots:
                nodes = dag.ancestors(headnodes)
            else:
                nodes = dag.range(rootnodes, headnodes)
        else:
            nodes = dag.reachableroots(rootnodes, headnodes)
        return list(self.torevs(nodes))

    def rawheadrevs(self):
        """Raw heads that exist in the changelog.
        This bypasses the visibility layer.

        This can be expensive and should be avoided if possible.
        """
        dag = self.dag
        heads = dag.headsancestors(dag.all())
        # Be compatible with C index headrevs: Return in ASC order.
        revs = self.torevs(heads)
        return list(revs.iterasc())

    def strip(self, minlink, transaction):
        # This should only be used in tests that uses 'debugstrip'
        if util.istest():
            raise error.Abort(_("strip is not supported"))
        # Invalidate on-disk nodemap.
        if self.indexfile.startswith("00changelog"):
            self.svfs.tryunlink("00changelog.nodemap")
            self.svfs.tryunlink("00changelog.i.nodemap")
        self.inner.strip([self.node(minlink)])

    def rev(self, node):
        if node == wdirid:
            raise error.WdirUnsupported
        try:
            return self.idmap.node2id(node)
        except error.CommitLookupError:
            raise error.LookupError(node, self.indexfile, _("no node"))

    def node(self, rev):
        if rev == wdirrev:
            raise error.WdirUnsupported
        try:
            return self.idmap.id2node(rev)
        except error.CommitLookupError:
            raise IndexError("revlog index out of range")

    def linkrev(self, rev):
        return rev

    def flags(self, rev):
        return 0

    def delayupdate(self, tr):
        pass

    def read(self, node):
        """Obtain data from a parsed changelog revision.

        Returns a 6-tuple of:

           - manifest node in binary
           - author/user as a localstr
           - date as a 2-tuple of (time, timezone)
           - list of files
           - commit message as a localstr
           - dict of extra metadata

        Unless you need to access all fields, consider calling
        ``changelogrevision`` instead, as it is faster for partial object
        access.
        """
        c = changelogrevision(self.revision(node))
        return (c.manifest, c.user, c.date, c.files, c.description, c.extra)

    def changelogrevision(self, nodeorrev):
        """Obtain a ``changelogrevision`` for a node or revision."""
        return changelogrevision(self.revision(nodeorrev))

    def readfiles(self, node):
        """
        short version of read that only returns the files modified by the cset
        """
        text = self.revision(node)
        return readfiles(text)

    def add(
        self, manifest, files, desc, transaction, p1, p2, user, date=None, extra=None
    ):
        text = hgcommittext(manifest, files, desc, user, date, extra)
        btext = encodeutf8(text)
        node = revlog.hash(btext, p1, p2)
        parents = [p for p in (p1, p2) if p != nullid]
        self.inner.addcommits([(node, parents, btext)])
        nodes = transaction.changes.get("nodes")
        if nodes is not None:
            nodes.append(node)
        return node

    def addgroup(self, deltas, linkmapper, transaction, addrevisioncb=None):
        nodes = []
        for node, p1, p2, linknode, deltabase, delta, flags in deltas:
            assert flags == 0, "changelog flags cannot be non-zero"
            parents = [p for p in (p1, p2) if p != nullid]
            basetext = self.revision(deltabase)
            rawtext = bytes(mdiff.patch(basetext, delta))
            self.inner.addcommits([(node, parents, rawtext)])
            if addrevisioncb:
                addrevisioncb(self, node)
            nodes.append(node)
        trnodes = transaction.changes.get("nodes")
        if trnodes is not None:
            trnodes += nodes
        return nodes

    def branchinfo(self, rev):
        """return the branch name and open/close state of a revision

        This function exists because creating a changectx object
        just to access this is costly."""
        extra = self.read(rev)[5]
        return encoding.tolocal(extra.get("branch")), "close" in extra

    def revision(self, nodeorrev, _df=None, raw=False):
        # type: (Union[int, bytes], Optional[IO], bool) -> bytes
        if nodeorrev in {nullid, nullrev}:
            return b""
        if isinstance(nodeorrev, bytes):
            node = nodeorrev
        else:
            node = self.node(nodeorrev)
        text = self.inner.getcommitrawtext(node)
        if text is None:
            raise error.LookupError(node, self.indexfile, _("no node"))
        return text

    def nodesbetween(self, roots, heads):
        """Calculate (roots::heads, roots & (roots::heads), heads & (roots::heads))"""
        result = self.dag.range(roots, heads)
        roots = roots & result
        heads = heads & result
        # Return in ASC order to be compatible with the old logic.
        return list(result.iterrev()), list(roots.iterrev()), list(heads.iterrev())

    def children(self, node):
        """Return children(node)"""
        nodes = self.dag.children([node])
        return list(nodes)

    def descendants(self, revs):
        """Return ((revs::) - roots(revs)) in revs."""
        dag = self.dag
        # nullrev special case.
        if nullrev in revs:
            result = dag.all()
        else:
            nodes = self.tonodes(revs)
            result = dag.descendants(nodes) - dag.roots(nodes)
        for rev in self.torevs(result).iterasc():
            yield rev

    def findcommonmissing(self, common, heads):
        """Return (torevs(::common), (::heads) - (::common))"""
        # "::heads - ::common" is "heads % common", aka. the "only"
        # operation.
        onlyheads, commonancestors = self.dag.onlyboth(heads, common)
        # commonancestors can be large, do not convert to list
        return self.torevs(commonancestors), list(onlyheads.iterrev())

    def findmissing(self, common, heads):
        """Return 'heads % common'"""
        return list(self.dag.only(heads, common).iterrev())

    def findmissingrevs(self, common, heads):
        tonodes = self.tonodes
        torevs = self.torevs
        return torevs(self.findmissing(tonodes(common), tonodes(heads)))

    def isancestor(self, a, b):
        """Test if a (in node) is an ancestor of b (in node)"""
        if a == nullid or b == nullid:
            return False
        return self.dag.isancestor(a, b)

    def ancestor(self, a, b):
        """Return the common ancestor, or nullid if there are no common
        ancestors.

        Common ancestors are defined as heads(::a & ::b).

        When there are multiple common ancestors, a "random" one is returned.
        """
        if nullid == a or nullid == b:
            return nullid
        return self.dag.gcaone([a, b]) or nullid

    def descendant(self, start, end):
        """Test if start (in rev) is an ancestor of end (in rev)"""
        return self.isancestor(self.node(start), self.node(end))

    def _partialmatch(self, hexprefix):
        matched = self.idmap.hexprefixmatch(hexprefix)
        if len(matched) > 1:
            # TODO: Add hints about possible matches.
            raise error.LookupError(
                hexprefix, self.indexfile, _("ambiguous identifier")
            )
        elif len(matched) == 1:
            return matched[0]
        else:
            return None

    def shortest(self, hexnode, minlength=1):
        def isvalid(test):
            try:
                if self._partialmatch(test) is None:
                    return False

                try:
                    i = int(test)
                    # if we are a pure int, then starting with zero will not be
                    # confused as a rev; or, obviously, if the int is larger
                    # than the value of the tip rev
                    if test[0] == "0" or i > len(self):
                        return True
                    return False
                except ValueError:
                    return True
            except error.RevlogError:
                return False
            except error.WdirUnsupported:
                # single 'ff...' match
                return True

        shortest = hexnode
        startlength = max(6, minlength)
        length = startlength
        while True:
            test = hexnode[:length]
            if isvalid(test):
                shortest = test
                if length == minlength or length > startlength:
                    return shortest
                length -= 1
            else:
                length += 1
                if len(shortest) <= length:
                    return shortest

    def ancestors(self, revs, stoprev=0, inclusive=False):
        """Return ::revs (in revs) if inclusive is True.

        If inclusive is False, return ::parents(revs).
        If stoprev is not zero, filter the result.
        stoprev is ignored in the Rust implementation.
        """
        nodes = self.tonodes(revs)
        dag = self.dag
        if not inclusive:
            nodes = dag.parents(nodes)
        ancestornodes = dag.ancestors(nodes)
        return self.torevs(ancestornodes)

    def commonancestorsheads(self, a, b):
        """Return heads(::a & ::b)"""
        # null special case
        if nullid == a or nullid == b:
            return []
        return list(self.dag.gcaall([a, b]))

    def parents(self, node):
        # special case for null
        if node == nullid:
            return (nullid, nullid)
        parents = list(self.dag.parentnames(node))
        while len(parents) < 2:
            parents.append(nullid)
        return parents

    def parentrevs(self, rev):
        return list(map(self.rev, self.parents(self.node(rev))))

    # Revlog-related APIs used in other places.

    def revdiff(self, rev1, rev2):
        """return or calculate a delta between two revisions"""
        return mdiff.textdiff(self.revision(rev1), self.revision(rev2))

    def deltaparent(self, rev):
        # Changelog does not have deltaparent
        return nullrev

    def iscensored(self, rev):
        return False

    def candelta(self, baserev, rev):
        return True


class nodemap(object):
    def __init__(self, changelog):
        self.changelog = changelog

    def __getitem__(self, node):
        rev = self.get(node)
        if rev is None:
            raise error.RevlogError(_("cannot find rev for %s") % hex(node))
        else:
            return rev

    def __setitem__(self, node, rev):
        if self.get(node) != rev:
            raise error.ProgrammingError("nodemap by Rust DAG is immutable")

    def __delitem__(self, node):
        raise error.ProgrammingError("nodemap by Rust DAG is immutable")

    def __contains__(self, node):
        return node in self.changelog.idmap

    def get(self, node, default=None):
        idmap = self.changelog.idmap
        if node not in idmap:
            return default
        else:
            return idmap.node2id(node)

    def destroying(self):
        pass


def migratetodoublewrite(repo):
    """Migrate to "double write" backend.

    Commit graph and IdMap use segments, commit text falls back to revlog.
    This can take about 1 minute for a large repo.
    """
    if "doublewritechangelog" in repo.storerequirements:
        return
    svfs = repo.svfs
    revlogdir = svfs.join("")
    segmentsdir = svfs.join(SEGMENTS_DIR)
    hgcommitsdir = svfs.join(HGCOMMITS_DIR)
    with repo.lock():
        master = list(repo.nodes("present(%s)", bookmod.mainbookmark(repo)))
        with progress.spinner(repo.ui, _("migrating commit graph")):
            bindings.dag.commits.migraterevlogtosegments(
                revlogdir, segmentsdir, hgcommitsdir, master
            )
        repo.storerequirements.discard("pythonrevlogchangelog")
        repo.storerequirements.discard("rustrevlogchangelog")
        repo.storerequirements.discard("segmentedchangelog")
        repo.storerequirements.add("doublewritechangelog")
        repo._writestorerequirements()
        repo.invalidatechangelog()


def migratetosegments(repo):
    """Migrate to full "segmentedchangelog" backend.

    Commit graph, IdMap, commit text are all backed by segmented changelog.
    This can take 10+ minutes for a large repo.
    """
    if "segmentedchangelog" in repo.storerequirements:
        return
    svfs = repo.svfs
    revlogdir = svfs.join("")
    segmentsdir = svfs.join(SEGMENTS_DIR)
    hgcommitsdir = svfs.join(HGCOMMITS_DIR)
    with repo.lock():
        zstore = bindings.zstore.zstore(svfs.join(HGCOMMITS_DIR))
        cl = repo.changelog
        with progress.bar(
            repo.ui, _("migrating commit text"), _("commits"), len(cl)
        ) as prog:
            clnode = cl.node
            clparents = cl.parents
            clrevision = cl.revision
            contains = zstore.__contains__
            insert = zstore.insert
            textwithheader = changelogmod.textwithheader
            try:
                for rev in cl.revs():
                    node = clnode(rev)
                    if contains(node):
                        continue
                    text = clrevision(rev)
                    p1, p2 = clparents(node)
                    newnode = insert(textwithheader(text, p1, p2))
                    assert node == newnode
                    prog.value += 1
            finally:
                # In case of Ctrl+C, flush commits in memory so we can continue
                # next time.
                zstore.flush()

        master = list(repo.nodes("present(%s)", bookmod.mainbookmark(repo)))
        with progress.spinner(repo.ui, _("migrating commit graph")):
            bindings.dag.commits.migraterevlogtosegments(
                revlogdir, segmentsdir, hgcommitsdir, master
            )

        repo.storerequirements.discard("doublewritechangelog")
        repo.storerequirements.discard("pythonrevlogchangelog")
        repo.storerequirements.discard("rustrevlogchangelog")
        repo.storerequirements.add("segmentedchangelog")
        repo._writestorerequirements()
        repo.invalidatechangelog()


def migratetorevlog(repo, python=False, rust=False):
    """Migrate to revlog backend.

    If python is True, set repo requirement to use Python + C revlog backend.
    If rust is True, set repo requirement to use Rust revlog backend.
    If neither is True, the backed is dynamically decided by the
    experimental.rust-commits config.
    """
    svfs = repo.svfs
    with repo.lock():
        # Migrate from segmentedchangelog
        if "segmentedchangelog" in repo.storerequirements:
            srccl = repo.changelog
            dstcl = changelog.openrevlog(svfs, repo.ui.uiconfig())
            with progress.bar(
                repo.ui, _("migrating commits"), _("commits"), len(srccl)
            ) as prog:
                hasnode = dstcl.hasnode
                getparents = srccl.dag.parentnames
                gettext = srccl.inner.getcommitrawtext
                addcommits = dstcl.inner.addcommits
                try:
                    for node in srccl.dag.all().iterrev():
                        if hasnode(node):
                            continue
                        parents = getparents(node)
                        btext = gettext(node)
                        addcommits([(node, parents, btext)])
                        prog.value += 1
                finally:
                    # In case of Ctrl+C, flush commits in memory so we can continue
                    # next time.
                    dstcl.inner.flush([])
        repo.storerequirements.discard("doublewritechangelog")
        repo.storerequirements.discard("pythonrevlogchangelog")
        repo.storerequirements.discard("rustrevlogchangelog")
        repo.storerequirements.discard("segmentedchangelog")
        if python:
            repo.storerequirements.add("pythonrevlogchangelog")
        if rust:
            repo.storerequirements.add("rustrevlogchangelog")
        repo._writestorerequirements()
        repo.invalidatechangelog()
