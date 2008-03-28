# hgweb/webutil.py - utility library for the web interface.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.node import hex, nullid
from mercurial.repo import RepoError
from mercurial import util

def siblings(siblings=[], hiderev=None, **args):
    siblings = [s for s in siblings if s.node() != nullid]
    if len(siblings) == 1 and siblings[0].rev() == hiderev:
        return
    for s in siblings:
        d = {'node': hex(s.node()), 'rev': s.rev()}
        if hasattr(s, 'path'):
            d['file'] = s.path()
        d.update(args)
        yield d

def renamelink(fl, node):
    r = fl.renamed(node)
    if r:
        return [dict(file=r[0], node=hex(r[1]))]
    return []

def nodetagsdict(repo, node):
    return [{"name": i} for i in repo.nodetags(node)]

def nodebranchdict(repo, ctx):
    branches = []
    branch = ctx.branch()
    # If this is an empty repo, ctx.node() == nullid,
    # ctx.branch() == 'default', but branchtags() is
    # an empty dict. Using dict.get avoids a traceback.
    if repo.branchtags().get(branch) == ctx.node():
        branches.append({"name": branch})
    return branches

def nodeinbranch(repo, ctx):
    branches = []
    branch = ctx.branch()
    if branch != 'default' and repo.branchtags().get(branch) != ctx.node():
        branches.append({"name": branch})
    return branches

def nodebranchnodefault(ctx):
    branches = []
    branch = ctx.branch()
    if branch != 'default':
        branches.append({"name": branch})
    return branches

def showtag(repo, tmpl, t1, node=nullid, **args):
    for t in repo.nodetags(node):
        yield tmpl(t1, tag=t, **args)

def cleanpath(repo, path):
    path = path.lstrip('/')
    return util.canonpath(repo.root, '', path)

def changectx(repo, req):
    if 'node' in req.form:
        changeid = req.form['node'][0]
    elif 'manifest' in req.form:
        changeid = req.form['manifest'][0]
    else:
        changeid = self.repo.changelog.count() - 1

    try:
        ctx = repo.changectx(changeid)
    except RepoError:
        man = repo.manifest
        mn = man.lookup(changeid)
        ctx = repo.changectx(man.linkrev(mn))

    return ctx

def filectx(repo, req):
    path = cleanpath(repo, req.form['file'][0])
    if 'node' in req.form:
        changeid = req.form['node'][0]
    else:
        changeid = req.form['filenode'][0]
    try:
        ctx = repo.changectx(changeid)
        fctx = ctx.filectx(path)
    except RepoError:
        fctx = repo.filectx(path, fileid=changeid)

    return fctx
