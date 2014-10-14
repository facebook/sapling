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

For more information and instructions, see :hg:`help git`
'''

from bisect import insort
import inspect
import os

from mercurial import bundlerepo
from mercurial import cmdutil
from mercurial import commands
from mercurial import demandimport
from mercurial import dirstate
from mercurial import discovery
from mercurial import exchange
from mercurial import extensions
from mercurial import help
from mercurial import hg
from mercurial import ignore
from mercurial import localrepo
from mercurial.node import hex
from mercurial import revset
from mercurial import scmutil
from mercurial import templatekw
from mercurial import util as hgutil
from mercurial import url
from mercurial.i18n import _

demandimport.ignore.extend([
    'collections',
    ])

import gitrepo, hgrepo, util
from git_handler import GitHandler
import verify

testedwith = '2.8.2 3.0.1 3.1'
buglink = 'https://bitbucket.org/durin42/hg-git/issues'

cmdtable = {}
command = cmdutil.command(cmdtable)

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
    if hgutil.safehasattr(lrepo, 'changelog') and co not in lrepo.changelog:
        co = None
    return revs, co
if getattr(hg, 'addbranchrevs', False):
    extensions.wrapfunction(hg, 'addbranchrevs', safebranchrevs)

def extsetup(ui):
    templatekw.keywords.update({'gitnode': gitnodekw})
    revset.symbols.update({
        'fromgit': revset_fromgit, 'gitnode': revset_gitnode
    })
    helpdir = os.path.join(os.path.dirname(__file__), 'help')
    entry = (['git'], _("Working with Git Repositories"),
        lambda: open(os.path.join(helpdir, 'git.rst')).read())
    insort(help.helptable, entry)

def reposetup(ui, repo):
    if not isinstance(repo, gitrepo.gitrepo):
        klass = hgrepo.generate_repo_subclass(repo.__class__)
        repo.__class__ = klass

@command('gimport')
def gimport(ui, repo, remote_name=None):
    '''import commits from Git to Mercurial'''
    repo.githandler.import_commits(remote_name)

@command('gexport')
def gexport(ui, repo):
    '''export commits from Mercurial to Git'''
    repo.githandler.export_commits()

@command('gclear')
def gclear(ui, repo):
    '''clear out the Git cached data'''
    repo.ui.status(_("clearing out the git cache data\n"))
    repo.githandler.clear()

@command('gverify',
         [('r', 'rev', '', _('revision to verify'), _('REV'))],
         _('[-r REV]'))
def gverify(ui, repo, **opts):
    '''verify that a Mercurial rev matches the corresponding Git rev

    Given a Mercurial revision that has a corresponding Git revision in the map,
    this attempts to answer whether that revision has the same contents as the
    corresponding Git revision.

    '''
    ctx = scmutil.revsingle(repo, opts.get('rev'), '.')
    return verify.verify(ui, repo, ctx)

if (getattr(dirstate, 'rootcache', False) and
    getattr(ignore, 'readpats', False)):
    # only install our dirstate wrapper if it has a hope of working
    import gitdirstate
    extensions.wrapfunction(ignore, 'ignore', gitdirstate.gignore)
    dirstate.dirstate = gitdirstate.gitdirstate

@command('git-cleanup')
def git_cleanup(ui, repo):
    '''clean up Git commit map after history editing'''
    new_map = []
    for line in repo.opener(GitHandler.map_file):
        gitsha, hgsha = line.strip().split(' ', 1)
        if hgsha in repo:
            new_map.append('%s %s\n' % (gitsha, hgsha))
    f = repo.opener(GitHandler.map_file, 'wb')
    map(f.write, new_map)
    ui.status(_('git commit map cleaned\n'))

def findcommonoutgoing(orig, repo, other, *args, **kwargs):
    if isinstance(other, gitrepo.gitrepo):
        heads = repo.githandler.get_refs(other.path)[0]
        kw = {}
        kw.update(kwargs)
        for val, k in zip(args,
                ('onlyheads', 'force', 'commoninc', 'portable')):
            kw[k] = val
        force = kw.get('force', False)
        commoninc = kw.get('commoninc', None)
        if commoninc is None:
            commoninc = discovery.findcommonincoming(repo, other,
                heads=heads, force=force)
            kw['commoninc'] = commoninc
        return orig(repo, other, **kw)
    return orig(repo, other, *args, **kwargs)
extensions.wrapfunction(discovery, 'findcommonoutgoing', findcommonoutgoing)

def getremotechanges(orig, ui, repo, other, *args, **opts):
    if isinstance(other, gitrepo.gitrepo):
        if args:
            revs = args[0]
        else:
            revs = opts.get('onlyheads', opts.get('revs'))
        r, c, cleanup = repo.githandler.getremotechanges(other, revs)
        # ugh. This is ugly even by mercurial API compatibility standards
        if 'onlyheads' not in orig.func_code.co_varnames:
            cleanup = None
        return r, c, cleanup
    return orig(ui, repo, other, *args, **opts)
extensions.wrapfunction(bundlerepo, 'getremotechanges', getremotechanges)

def peer(orig, uiorrepo, *args, **opts):
    newpeer = orig(uiorrepo, *args, **opts)
    if isinstance(newpeer, gitrepo.gitrepo):
        if isinstance(uiorrepo, localrepo.localrepository):
            newpeer.localrepo = uiorrepo
    return newpeer
extensions.wrapfunction(hg, 'peer', peer)

@util.transform_notgit
def exchangepull(orig, repo, remote, heads=None, force=False, bookmarks=()):
    if isinstance(remote, gitrepo.gitrepo):
        pullop = exchange.pulloperation(repo, remote, heads, force,
                                        bookmarks=bookmarks)
        lock = repo.lock()
        try:
            pullop.cgresult = repo.githandler.fetch(remote.path, heads)
            pullop.closetransaction()
            return pullop
        finally:
            pullop.releasetransaction()
            lock.release()
    else:
        return orig(repo, remote, heads, force, bookmarks=bookmarks)
if not hgutil.safehasattr(localrepo.localrepository, 'pull'):
    # Mercurial >= 3.2
    extensions.wrapfunction(exchange, 'pull', exchangepull)

def revset_fromgit(repo, subset, x):
    '''``fromgit()``
    Select changesets that originate from Git.
    '''
    args = revset.getargs(x, 0, 0, "fromgit takes no arguments")
    git = repo.githandler
    node = repo.changelog.node
    return [r for r in subset if git.map_git_get(hex(node(r))) is not None]

def revset_gitnode(repo, subset, x):
    '''``gitnode(hash)``
    Select changesets that originate in the given Git revision.
    '''
    args = revset.getargs(x, 1, 1, "gitnode takes one argument")
    rev = revset.getstring(args[0],
                           "the argument to gitnode() must be a hash")
    git = repo.githandler
    node = repo.changelog.node
    def matches(r):
        gitnode = git.map_git_get(hex(node(r)))
        if gitnode is None:
            return False
        return rev in [gitnode, gitnode[:12]]
    return [r for r in subset if matches(r)]

def gitnodekw(**args):
    """:gitnode: String.  The Git changeset identification hash, as a 40 hexadecimal digit string."""
    node = args['ctx']
    repo = args['repo']
    gitnode = repo.githandler.map_git_get(node.hex())
    if gitnode is None:
        gitnode = ''
    return gitnode
