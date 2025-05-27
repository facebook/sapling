# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Olivia Mackall
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import hashlib
from collections import defaultdict
from typing import List, Optional

from . import bookmarks, bundle2, changegroup, discovery, error, scmutil, util
from .i18n import _
from .node import bin, hex, short


def _bundle(repo, bases, heads, node, suffix, compress: bool = True) -> str:
    """create a bundle with the specified revisions as a backup"""

    backupdir = "strip-backup"
    vfs = repo.localvfs
    if not vfs.isdir(backupdir):
        vfs.mkdir(backupdir)

    # Include a hash of all the nodes in the filename for uniqueness
    allcommits = repo.set("%ln::%ln", bases, heads)
    allhashes = sorted(c.hex() for c in allcommits)
    totalhash = hashlib.sha1("".join(allhashes).encode()).digest()
    name = "%s/%s-%s-%s.hg" % (backupdir, short(node), hex(totalhash[:4]), suffix)

    cgversion = changegroup.localversion(repo)
    comp = None
    if cgversion != "01":
        bundletype = "HG20"
        if compress:
            comp = "BZ"
    elif compress:
        bundletype = "HG10BZ"
    else:
        bundletype = "HG10UN"

    outgoing = discovery.outgoing(repo, missingroots=bases, missingheads=heads)
    contentopts = {
        "cg.version": cgversion,
        "phases": True,
    }
    return bundle2.writenewbundle(
        repo.ui,
        repo,
        "strip",
        name,
        bundletype,
        outgoing,
        contentopts,
        vfs,
        compression=comp,
    )


def _collectfiles(repo, striprev):
    """find out the filelogs affected by the strip"""
    files = set()

    for x in range(striprev, len(repo)):
        files.update(repo[x].files())

    return sorted(files)


def _collectrevlog(revlog, striprev):
    _, brokenset = revlog.getstrippoint(striprev)
    return [revlog.linkrev(r) for r in brokenset]


def _collectmanifest(repo, striprev):
    return _collectrevlog(repo.manifestlog._revlog, striprev)


def _collectbrokencsets(repo, files, striprev):
    """return the changesets which will be broken by the truncation"""
    s = set()

    s.update(_collectmanifest(repo, striprev))
    for fname in files:
        s.update(_collectrevlog(repo.file(fname), striprev))

    return s


def stripgeneric(repo, nodelist, backup: bool = True, topic: str = "backup") -> None:
    """Strip that does not depend on revlog details, namely:

    - Do not use non-DAG span "rev:".
    - Give up dealing with linkrevs which are specific to revlog.

    This is only be used in legacy tests for compatibility.
    Non-test uses are forbidden.
    Do not rely on this for new code.
    """
    assert util.istest()

    with repo.lock():
        # Give up on linkrevs handling by just saying all linkrevs are
        # invalidated now.
        repo.storerequirements.add("invalidatelinkrev")
        repo._writestorerequirements()

        # Generate backup.
        if backup:
            _bundle(repo, nodelist, repo.heads(), nodelist[-1], topic)

        # Apply bookmark and visibility changes.
        with repo.transaction("strip"):
            allnodes = list(repo.nodes("%ln::", nodelist))
            scmutil.cleanupnodes(repo, allnodes, "strip")

            # Dumb hack to roll remotenames backwards (helps compat with remotenames ext).
            # Things don't work when you have a remote bookmark pointing to a commit not
            # in the local commit graph, so walk p1 until we find non-stripped commit.
            remotenames = defaultdict(dict)
            for hexbm, typ, remote, name in bookmarks.readremotenames(repo):
                while bin(hexbm) in allnodes:
                    hexbm = repo[hexbm].p1().hex()
                remotenames[remote][name] = hexbm
            bookmarks.saveremotenames(repo, remotenames)

        # Strip changelog (unsafe for readers).
        # Handled by the Rust layer. Independent from revlog.
        repo.changelog.inner.strip(nodelist)

        # Since we give up on linkrevs, it's fine to have
        # unreferenced manifest or file revisions. No need
        # to strip them.


def strip(
    ui, repo, nodelist: List[str], backup: bool = True, topic: str = "backup"
) -> None:
    # This function requires the caller to lock the repo, but it operates
    # within a transaction of its own, and thus requires there to be no current
    # transaction when it is called.
    if repo.currenttransaction() is not None:
        raise error.ProgrammingError("cannot strip from inside a transaction")

    if isinstance(nodelist, str):
        nodelist = [nodelist]

    # Simple way to maintain backwards compatibility for this
    # argument.
    if backup in ["none", "strip"]:
        backup = False

    return stripgeneric(repo, nodelist, backup, topic)


def safestriproots(ui, repo, nodes):
    """return list of roots of nodes where descendants are covered by nodes"""
    torev = repo.changelog.rev
    revs = set(torev(n) for n in nodes)
    # tostrip = wanted - unsafe = wanted - ancestors(orphaned)
    # orphaned = affected - wanted
    # affected = descendants(roots(wanted))
    # wanted = revs
    tostrip = set(repo.revs("%ld-(::((roots(%ld)::)-%ld))", revs, revs, revs))
    notstrip = revs - tostrip
    if notstrip:
        nodestr = ", ".join(sorted(short(repo[n].node()) for n in notstrip))
        ui.warn(
            _("warning: orphaned descendants detected, not stripping %s\n") % nodestr
        )
    return [c.node() for c in repo.set("roots(%ld)", tostrip)]


class stripcallback:
    """used as a transaction postclose callback"""

    def __init__(self, ui, repo, backup, topic):
        self.ui = ui
        self.repo = repo
        self.backup = backup
        self.topic = topic or "backup"
        self.nodelist = []

    def addnodes(self, nodes):
        self.nodelist.extend(nodes)

    def __call__(self, tr):
        roots = safestriproots(self.ui, self.repo, self.nodelist)
        if roots:
            strip(self.ui, self.repo, roots, self.backup, self.topic)


def delayedstrip(ui, repo, nodelist, topic: Optional[str] = None) -> None:
    """like strip, but works inside transaction and won't strip irreverent revs

    nodelist must explicitly contain all descendants. Otherwise a warning will
    be printed that some nodes are not stripped.

    Always do a backup. The last non-None "topic" will be used as the backup
    topic name. The default backup topic name is "backup".
    """
    tr = repo.currenttransaction()
    if not tr:
        nodes = safestriproots(ui, repo, nodelist)
        # pyre-fixme[6]: For 5th param expected `str` but got `Optional[str]`.
        return strip(ui, repo, nodes, True, topic)
    # transaction postclose callbacks are called in alphabet order.
    # use '\xff' as prefix so we are likely to be called last.
    callback = tr.getpostclose("\xffstrip")
    if callback is None:
        callback = stripcallback(ui, repo, True, topic)
        tr.addpostclose("\xffstrip", callback)
    if topic:
        callback.topic = topic
    callback.addnodes(nodelist)


def stripmanifest(repo, striprev, tr, files) -> None:
    revlog = repo.manifestlog._revlog
    revlog.strip(striprev, tr)
    striptrees(repo, tr, striprev, files)


def striptrees(repo, tr, striprev, files) -> None:
    pass
