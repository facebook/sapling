# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import hex
import encoding

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
