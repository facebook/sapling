# git.py - git server bridge
#
# Copyright 2008 Scott Chacon <schacon at gmail dot com>
#   also some code (and help) borrowed from durin42
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

'''push and pull from a Git server

This extension lets you communicate (push and pull) with a Git server.
This way you can use Git hosting for your project or collaborate with a
project that is in Git.  A bridger of worlds, this plugin be.

Try hg clone git:// or hg clone git+ssh://
'''

import inspect
import os

from mercurial import bundlerepo
from mercurial import commands
from mercurial import demandimport
from mercurial import extensions
from mercurial import hg
from mercurial import localrepo
from mercurial import util as hgutil
from mercurial import url
from mercurial.i18n import _

demandimport.ignore.extend([
    'collections',
    ])

import gitrepo, hgrepo
from git_handler import GitHandler

# support for `hg clone git://github.com/defunkt/facebox.git`
# also hg clone git+ssh://git@github.com/schacon/simplegit.git
_gitschemes = ('git', 'git+ssh', 'git+http', 'git+https')
for _scheme in _gitschemes:
    hg.schemes[_scheme] = gitrepo

# support for `hg clone localgitrepo`
_oldlocal = hg.schemes['file']

try:
    urlcls = hgutil.url
except AttributeError:
    class urlcls(object):
        def __init__(self, path):
            self.p = hgutil.drop_scheme('file', path)

        def localpath(self):
            return self.p

def _local(path):
    p = urlcls(path).localpath()
    if (os.path.exists(os.path.join(p, '.git')) and
        not os.path.exists(os.path.join(p, '.hg'))):
        return gitrepo
    # detect a bare repository
    if (os.path.exists(os.path.join(p, 'HEAD')) and
        os.path.exists(os.path.join(p, 'objects')) and
        os.path.exists(os.path.join(p, 'refs')) and
        not os.path.exists(os.path.join(p, '.hg'))):
        return gitrepo
    return _oldlocal(path)

hg.schemes['file'] = _local

hgdefaultdest = hg.defaultdest
def defaultdest(source):
    for scheme in _gitschemes:
        if source.startswith('%s://' % scheme) and source.endswith('.git'):
            source = source[:-4]
            break
    return hgdefaultdest(source)
hg.defaultdest = defaultdest

# defend against tracebacks if we specify -r in 'hg pull'
def safebranchrevs(orig, lrepo, repo, branches, revs):
    revs, co = orig(lrepo, repo, branches, revs)
    if getattr(lrepo, 'changelog', False) and co not in lrepo.changelog:
        co = None
    return revs, co
if getattr(hg, 'addbranchrevs', False):
    extensions.wrapfunction(hg, 'addbranchrevs', safebranchrevs)

def reposetup(ui, repo):
    if not isinstance(repo, gitrepo.gitrepo):
        klass = hgrepo.generate_repo_subclass(repo.__class__)
        repo.__class__ = klass

def gimport(ui, repo, remote_name=None):
    git = GitHandler(repo, ui)
    git.import_commits(remote_name)

def gexport(ui, repo):
    git = GitHandler(repo, ui)
    git.export_commits()

def gclear(ui, repo):
    repo.ui.status(_("clearing out the git cache data\n"))
    git = GitHandler(repo, ui)
    git.clear()

def git_cleanup(ui, repo):
    new_map = []
    for line in repo.opener(GitHandler.mapfile):
        gitsha, hgsha = line.strip().split(' ', 1)
        if hgsha in repo:
            new_map.append('%s %s\n' % (gitsha, hgsha))
    f = repo.opener(GitHandler.mapfile, 'wb')
    map(f.write, new_map)
    ui.status(_('git commit map cleaned\n'))

# drop this when we're 1.6-only, this just backports new behavior
def sortednodetags(orig, *args, **kwargs):
    ret = orig(*args, **kwargs)
    ret.sort()
    return ret
extensions.wrapfunction(localrepo.localrepository, 'nodetags', sortednodetags)

try:
    from mercurial import discovery
    kwname = 'heads'
    if hg.util.version() >= '1.7':
        kwname = 'remoteheads'
    if getattr(discovery, 'findcommonoutgoing', None):
        kwname = 'onlyheads'
    def findoutgoing(orig, local, remote, *args, **kwargs):
        if isinstance(remote, gitrepo.gitrepo):
            raise hgutil.Abort(
                'hg-git outgoing support is broken')
        return orig(local, remote, *args, **kwargs)
    if getattr(discovery, 'findoutgoing', None):
        extensions.wrapfunction(discovery, 'findoutgoing', findoutgoing)
    else:
        extensions.wrapfunction(discovery, 'findcommonoutgoing',
                                findoutgoing)
except ImportError:
    pass

def getremotechanges(orig, ui, repo, other, *args, **opts):
    if isinstance(other, gitrepo.gitrepo):
        if args:
            revs = args[0]
        else:
            revs = opts.get('onlyheads', opts.get('revs'))
        git = GitHandler(repo, ui)
        r, c, cleanup = git.getremotechanges(other, revs)
        # ugh. This is ugly even by mercurial API compatibility standards
        if 'onlyheads' not in orig.func_code.co_varnames:
            cleanup = None
        return r, c, cleanup
    return orig(ui, repo, other, *args, **opts)
try:
    extensions.wrapfunction(bundlerepo, 'getremotechanges', getremotechanges)
except AttributeError:
    # 1.7+
    pass

cmdtable = {
  "gimport":
        (gimport, [], _('hg gimport')),
  "gexport":
        (gexport, [], _('hg gexport')),
  "gclear":
      (gclear, [], _('Clears out the Git cached data')),
  "git-cleanup": (git_cleanup, [], _(
        "Cleans up git repository after history editing"))
}
