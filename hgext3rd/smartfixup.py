# smartfixup.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""apply working directory changes to changesets

The smartfixup extension provides a command to use annotate information to
amend modified chunks into the corresponding non-public changesets.

::

    [smartfixup]
    # only check 50 recent non-public changesets at most
    maxstacksize = 50
    # whether to add noise to new commits to avoid obsolescence cycle
    addnoise = 1

    [color]
    smartfixup.node = blue bold
    smartfixup.path = bold
"""

from __future__ import absolute_import

from collections import defaultdict
import linelog

from mercurial import (
    cmdutil,
    commands,
    context,
    crecord,
    error,
    mdiff,
    node,
    obsolete,
    patch,
    phases,
    repair,
    scmutil,
    util,
)
from mercurial.i18n import _

testedwith = 'internal'

cmdtable = {}
command = cmdutil.command(cmdtable)

class nullui(object):
    """blank ui object doing nothing"""
    debugflag = False
    verbose = False
    quiet = True

    def __getitem__(name):
        def nullfunc(*args, **kwds):
            return
        return nullfunc

class emptyfilecontext(object):
    """minimal filecontext representing an empty file"""
    def data(self):
        return ''

    def node(self):
        return node.nullid

def uniq(lst):
    """list -> list. remove duplicated items without changing the order"""
    seen = set()
    result = []
    for x in lst:
        if x not in seen:
            seen.add(x)
            result.append(x)
    return result

def getdraftstack(headctx, limit=None):
    """(ctx, int?) -> [ctx]. get a linear stack of non-public changesets.

    changesets are sorted in topo order, oldest first.
    return at most limit items, if limit is a positive number.

    merges are considered as non-draft as well. i.e. every commit
    returned has and only has 1 parent.
    """
    ctx = headctx
    result = []
    while ctx.phase() != phases.public:
        if limit and len(result) >= limit:
            break
        parents = ctx.parents()
        if len(parents) != 1:
            break
        result.append(ctx)
        ctx = parents[0]
    result.reverse()
    return result

class overlaystore(object):
    """read-only, hybrid store based on a dict and ctx.
    memworkingcopy: {path: content}, overrides file contents.
    """
    def __init__(self, basectx, memworkingcopy):
        self.basectx = basectx
        self.memworkingcopy = memworkingcopy

    def getfile(self, path):
        """comply with mercurial.patch.filestore.getfile"""
        fctx = self.basectx[path]
        if path in self.memworkingcopy:
            content = self.memworkingcopy[path]
        else:
            content = fctx.data()
        mode = (fctx.islink(), fctx.isbinary())
        renamed = fctx.renamed() # False or (path, node)
        return content, mode, (renamed and renamed[0])

def overlaycontext(memworkingcopy, ctx, parents=None, extra=None):
    """({path: content}, ctx, (p1node, p2node)?, {}?) -> memctx
    memworkingcopy overrides file contents.
    """
    # parents must contain 2 items: (node1, node2)
    if parents is None:
        parents = ctx.repo().changelog.parents(ctx.node())
    if extra is None:
        extra = ctx.extra()
    date = ctx.date()
    desc = ctx.description()
    user = ctx.user()
    files = set(ctx.files()).union(memworkingcopy.iterkeys())
    store = overlaystore(ctx, memworkingcopy)
    return context.makememctx(ctx.repo(), parents, desc, user, date, None,
                              files, store, extra=extra)
