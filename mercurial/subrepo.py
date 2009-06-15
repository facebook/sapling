# subrepo.py - sub-repository handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import errno, os
import config, util, node, error
localrepo = None

nullstate = ('', '')

def state(ctx):
    p = config.config()
    def read(f, sections=None, remap=None):
        if f in ctx:
            try:
                p.parse(f, ctx[f].data(), sections, remap)
            except IOError, err:
                if err.errno != errno.ENOENT:
                    raise
    read('.hgsub')

    rev = {}
    if '.hgsubstate' in ctx:
        try:
            for l in ctx['.hgsubstate'].data().splitlines():
                revision, path = l.split()
                rev[path] = revision
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise

    state = {}
    for path, src in p[''].items():
        state[path] = (src, rev.get(path, ''))

    return state

def writestate(repo, state):
    repo.wwrite('.hgsubstate',
                ''.join(['%s %s\n' % (state[s][1], s)
                         for s in sorted(state)]), '')

def subrepo(ctx, path):
    # subrepo inherently violates our import layering rules
    # because it wants to make repo objects from deep inside the stack
    # so we manually delay the circular imports to not break
    # scripts that don't use our demand-loading
    global localrepo
    import localrepo as l
    localrepo = l

    state = ctx.substate.get(path, nullstate)
    if state[0].startswith('['): # future expansion
        raise error.Abort('unknown subrepo source %s' % state[0])
    return hgsubrepo(ctx, path, state)

class hgsubrepo(object):
    def __init__(self, ctx, path, state):
        self._parent = ctx
        self._path = path
        self._state = state
        r = ctx._repo
        root = r.wjoin(path)
        self._repo = localrepo.localrepository(r.ui, root)

    def dirty(self):
        r = self._state[1]
        if r == '':
            return True
        w = self._repo[None]
        if w.p1() != self._repo[r]: # version checked out changed
            return True
        return w.dirty() # working directory changed

    def commit(self, text, user, date):
        n = self._repo.commit(text, user, date)
        if not n:
            return self._repo['.'].hex() # different version checked out
        return node.hex(n)
