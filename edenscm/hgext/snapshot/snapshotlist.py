# snapshotlist.py - list of local snapshots
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from edenscm.mercurial import error, localrepo, node, pycompat, txnutil
from edenscm.mercurial.i18n import _


# Supported file format version.
# Version 1 is:
#  * A single line containing "v1"
#  * A list of node hashes for each snapshot, one per line.
FORMAT_VERSION = "v1"


def reposetup(ui, repo):
    class snapshotrepo(repo.__class__):
        @localrepo.storecache("snapshotlist")
        def snapshotlist(self):
            return snapshotlist(self)

    repo.__class__ = snapshotrepo


def _getsnapshotlistfile(repo):
    fp, pending = txnutil.trypending(repo.root, repo.svfs, "snapshotlist")
    return fp


class snapshotlist(object):
    """list of local snapshots (hex nodes)

    It is used by `hg snapshot list` command.
    The nodes from this file are returned by the `snapshot()` revset expression.
    E.g. only these nodes are synced to CommitCloud.

    Locally is stored in `.hg/store/snapshotlist`.
    """

    def __init__(self, repo, check=True):
        try:
            with _getsnapshotlistfile(repo) as snaplistfile:
                lines = snaplistfile.readlines()
            if not lines or lines[0].strip() != FORMAT_VERSION:
                raise error.Abort("invalid snapshots file format")
            self.snapshots = [snapshot.strip() for snapshot in lines[1:]]
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            self.snapshots = []
        if check:
            self._check(repo)

    def _check(self, repo):
        unfi = repo.unfiltered()
        toremove = set()
        for snapshotnode in self.snapshots:
            binsnapshotnode = node.bin(snapshotnode)
            if binsnapshotnode not in unfi:
                raise error.Abort("invalid snapshot node: %s" % snapshotnode)
            if "snapshotmetadataid" not in unfi[binsnapshotnode].extra():
                toremove.add(snapshotnode)
        if len(toremove) != 0:
            self.snapshots = [s for s in self.snapshots if s not in toremove]

    def _write(self, fp):
        fp.write("%s\n" % FORMAT_VERSION)
        for s in self.snapshots:
            fp.write("%s\n" % (s,))

    def update(self, tr, addnodes=[], removenodes=[]):
        """transactionally update the list of snapshots

        Elements of addnodes and removenodes are expected to be binary nodes"""
        nodes = set(self.snapshots)
        toadd = set(addnodes) - nodes
        toremove = set(removenodes) & nodes
        if len(toadd) != 0 or len(toremove) != 0:
            self.snapshots = [s for s in self.snapshots if s not in toremove] + list(
                sorted(addnodes)
            )
            tr.addfilegenerator("snapshots", ("snapshotlist",), self._write)

    def printsnapshots(self, ui, repo, **opts):
        opts = pycompat.byteskwargs(opts)
        fm = ui.formatter("snapshots", opts)
        if len(self.snapshots) == 0:
            ui.status(_("no snapshots created\n"))
        unfi = repo.unfiltered()
        for snapshotnode in self.snapshots:
            ctx = unfi[snapshotnode]
            message = ctx.description().split("\n")[0]
            metadataid = ctx.extra()["snapshotmetadataid"]
            if metadataid:
                metadataid = metadataid[:12]
            else:
                metadataid = "None"

            fm.startitem()
            # TODO(alexeyqu): print list of related files if --verbose
            fm.write("revision", "%s", str(ctx))
            fm.condwrite(ui.verbose, "snapshotmetadataid", "% 15s", metadataid)
            fm.write("message", " %s", message)
            fm.plain("\n")
        fm.end()
