# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import bin, hex, nullid, nullrev
import encoding

def read(repo):
    partial = {}
    try:
        f = repo.opener("cache/branchheads")
        lines = f.read().split('\n')
        f.close()
    except (IOError, OSError):
        return {}, nullid, nullrev

    try:
        last, lrev = lines.pop(0).split(" ", 1)
        last, lrev = bin(last), int(lrev)
        if lrev >= len(repo) or repo[lrev].node() != last:
            # invalidate the cache
            raise ValueError('invalidating branch cache (tip differs)')
        for l in lines:
            if not l:
                continue
            node, label = l.split(" ", 1)
            label = encoding.tolocal(label.strip())
            if not node in repo:
                raise ValueError('invalidating branch cache because node '+
                                 '%s does not exist' % node)
            partial.setdefault(label, []).append(bin(node))
    except KeyboardInterrupt:
        raise
    except Exception, inst:
        if repo.ui.debugflag:
            repo.ui.warn(str(inst), '\n')
        partial, last, lrev = {}, nullid, nullrev
    return partial, last, lrev

def write(repo, branches, tip, tiprev):
    try:
        f = repo.opener("cache/branchheads", "w", atomictemp=True)
        f.write("%s %s\n" % (hex(tip), tiprev))
        for label, nodes in branches.iteritems():
            for node in nodes:
                f.write("%s %s\n" % (hex(node), encoding.fromlocal(label)))
        f.close()
    except (IOError, OSError):
        pass
