# bruterebase.py - brute force rebase testing
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    error,
    registrar,
    revsetlang,
)

from hgext import rebase

try:
    xrange
except NameError:
    xrange = range

cmdtable = {}
command = registrar.command(cmdtable)

@command(b'debugbruterebase')
def debugbruterebase(ui, repo, source, dest):
    """for every non-empty subset of source, run rebase -r subset -d dest

    Print one line summary for each subset. Assume obsstore is enabled.
    """
    srevs = list(repo.revs(source))

    with repo.wlock(), repo.lock():
        repolen = len(repo)
        cl = repo.changelog

        def getdesc(rev):
            result = cl.changelogrevision(rev).description
            if rev >= repolen:
                result += b"'"
            return result

        for i in xrange(1, 2 ** len(srevs)):
            subset = [rev for j, rev in enumerate(srevs) if i & (1 << j) != 0]
            spec = revsetlang.formatspec(b'%ld', subset)
            tr = repo.transaction(b'rebase')
            tr.report = lambda x: 0 # hide "transaction abort"

            ui.pushbuffer()
            try:
                rebase.rebase(ui, repo, dest=dest, rev=[spec])
            except error.Abort as ex:
                summary = b'ABORT: %s' % ex
            except Exception as ex:
                summary = b'CRASH: %s' % ex
            else:
                # short summary about new nodes
                cl = repo.changelog
                descs = []
                for rev in xrange(repolen, len(repo)):
                    desc = b'%s:' % getdesc(rev)
                    for prev in cl.parentrevs(rev):
                        if prev > -1:
                            desc += getdesc(prev)
                    descs.append(desc)
                descs.sort()
                summary = ' '.join(descs)
            ui.popbuffer()
            repo.vfs.tryunlink(b'rebasestate')

            subsetdesc = b''.join(getdesc(rev) for rev in subset)
            ui.write((b'%s: %s\n') % (subsetdesc.rjust(len(srevs)), summary))
            tr.abort()
