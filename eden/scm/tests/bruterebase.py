# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm import error, registrar, revsetlang

from edenscm.ext import rebase


try:
    xrange
except NameError:
    xrange = range

cmdtable = {}
command = registrar.command(cmdtable)


@command("debugbruterebase")
def debugbruterebase(ui, repo, source, dest):
    """for every non-empty subset of source, run rebase -r subset -d dest

    Print one line summary for each subset. Assume obsstore is enabled.
    """
    srevs = list(repo.revs(source))

    with repo.wlock(), repo.lock():
        repolen = len(repo)
        cl = repo.changelog
        newrevs = []

        def getdesc(rev):
            result = cl.changelogrevision(rev).description
            if rev in newrevs:
                result += "'"
            return result

        for i in xrange(1, 2 ** len(srevs)):
            subset = [rev for j, rev in enumerate(srevs) if i & (1 << j) != 0]
            spec = revsetlang.formatspec("%ld", subset)
            tr = repo.transaction("rebase")
            tr.report = lambda x: 0  # hide "transaction abort"

            oldnodes = set(repo.nodes("all()"))
            ui.pushbuffer()
            try:
                rebase.rebase(ui, repo, dest=dest, rev=[spec])
            except error.Abort as ex:
                summary = "ABORT: %s" % ex
            except Exception as ex:
                summary = "CRASH: %s" % ex
            else:
                # short summary about new nodes
                cl = repo.changelog
                descs = []
                newnodes = set(repo.nodes("all()"))
                newrevs = list(map(cl.rev, newnodes - oldnodes))
                for rev in sorted(newrevs):
                    desc = "%s:" % getdesc(rev)
                    for prev in cl.parentrevs(rev):
                        if prev > -1:
                            desc += getdesc(prev)
                    descs.append(desc)
                descs.sort()
                summary = " ".join(descs)
            ui.popbuffer()
            repo.localvfs.tryunlink("rebasestate")

            subsetdesc = "".join(getdesc(rev) for rev in subset)
            ui.write(("%s: %s\n") % (subsetdesc.rjust(len(srevs)), summary))
            tr.abort()
