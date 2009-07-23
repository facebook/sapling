# subrepo.py - sub-repository handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import errno, os
from i18n import _
import config, util, node, error
hg = None

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

def submerge(repo, wctx, mctx, actx):
    if mctx == actx: # backwards?
        actx = wctx.p1()
    s1 = wctx.substate
    s2 = mctx.substate
    sa = actx.substate
    sm = {}

    for s, l in s1.items():
        a = sa.get(s, nullstate)
        if s in s2:
            r = s2[s]
            if l == r or r == a: # no change or local is newer
                sm[s] = l
                continue
            elif l == a: # other side changed
                wctx.sub(s).get(r)
                sm[s] = r
            elif l[0] != r[0]: # sources differ
                if repo.ui.promptchoice(
                    _(' subrepository sources for %s differ\n'
                      'use (l)ocal source (%s) or (r)emote source (%s)?')
                      % (s, l[0], r[0]),
                      (_('&Local'), _('&Remote')), 0):
                    wctx.sub(s).get(r)
                    sm[s] = r
            elif l[1] == a[1]: # local side is unchanged
                wctx.sub(s).get(r)
                sm[s] = r
            else:
                wctx.sub(s).merge(r)
                sm[s] = l
        elif l == a: # remote removed, local unchanged
            wctx.sub(s).remove()
        else:
            if repo.ui.promptchoice(
                _(' local changed subrepository %s which remote removed\n'
                  'use (c)hanged version or (d)elete?') % s,
                (_('&Changed'), _('&Delete')), 0):
                wctx.sub(s).remove()

    for s, r in s2.items():
        if s in s1:
            continue
        elif s not in sa:
            wctx.sub(s).get(r)
            sm[s] = r
        elif r != sa[s]:
            if repo.ui.promptchoice(
                _(' remote changed subrepository %s which local removed\n'
                  'use (c)hanged version or (d)elete?') % s,
                (_('&Changed'), _('&Delete')), 0) == 0:
                wctx.sub(s).get(r)
                sm[s] = r

    # record merged .hgsubstate
    writestate(repo, sm)

def _abssource(repo, push=False):
    if hasattr(repo, '_subparent'):
        source = repo._subsource
        if source.startswith('/') or '://' in source:
            return source
        parent = _abssource(repo._subparent)
        if '://' in parent:
            if parent[-1] == '/':
                parent = parent[:-1]
            return parent + '/' + source
        return os.path.join(parent, repo._subsource)
    if push and repo.ui.config('paths', 'default-push'):
        return repo.ui.config('paths', 'default-push', repo.root)
    return repo.ui.config('paths', 'default', repo.root)

def subrepo(ctx, path):
    # subrepo inherently violates our import layering rules
    # because it wants to make repo objects from deep inside the stack
    # so we manually delay the circular imports to not break
    # scripts that don't use our demand-loading
    global hg
    import hg as h
    hg = h

    util.path_auditor(ctx._repo.root)(path)
    state = ctx.substate.get(path, nullstate)
    if state[0].startswith('['): # future expansion
        raise error.Abort('unknown subrepo source %s' % state[0])
    return hgsubrepo(ctx, path, state)

class hgsubrepo(object):
    def __init__(self, ctx, path, state):
        self._path = path
        self._state = state
        r = ctx._repo
        root = r.wjoin(path)
        if os.path.exists(os.path.join(root, '.hg')):
            self._repo = hg.repository(r.ui, root)
        else:
            util.makedirs(root)
            self._repo = hg.repository(r.ui, root, create=True)
        self._repo._subparent = r
        self._repo._subsource = state[0]

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

    def remove(self):
        # we can't fully delete the repository as it may contain
        # local-only history
        self._repo.ui.note(_('removing subrepo %s\n') % self._path)
        hg.clean(self._repo, node.nullid, False)

    def get(self, state):
        source, revision = state
        try:
            self._repo.lookup(revision)
        except error.RepoError:
            self._repo._subsource = source
            self._repo.ui.status(_('pulling subrepo %s\n') % self._path)
            srcurl = _abssource(self._repo)
            other = hg.repository(self._repo.ui, srcurl)
            self._repo.pull(other)

        hg.clean(self._repo, revision, False)

    def merge(self, state):
        hg.merge(self._repo, state[1], remind=False)

    def push(self, force):
        # push subrepos depth-first for coherent ordering
        c = self._repo['']
        subs = c.substate # only repos that are committed
        for s in sorted(subs):
            c.sub(s).push(force)

        self._repo.ui.status(_('pushing subrepo %s\n') % self._path)
        dsturl = _abssource(self._repo, True)
        other = hg.repository(self._repo.ui, dsturl)
        self._repo.push(other, force)

