from mercurial import hg
from mercurial import templatekw
from mercurial import revset
from mercurial.i18n import _

def showgitnode(repo, ctx, templ, **args):
    """Return the git revision corresponding to a given hg rev"""
    peerpath = repo.ui.expandpath('default')
    remoterepo = hg.peer(repo, {}, peerpath)
    remoterev = remoterepo.lookup('_gitlookup_hg_%s' % ctx.hex())
    return remoterev.encode('hex')

def gitnode(repo, subset, x):
    """Return the hg revision corresponding to a given git rev"""
    l = revset.getargs(x, 1, 1, _("id requires one argument"))
    n = revset.getstring(l[0], _("id requires a string"))
    peerpath = repo.ui.expandpath('default')
    remoterepo = hg.peer(repo, {}, peerpath)
    remoterev = remoterepo.lookup('_gitlookup_git_%s' % n)
    rn = repo[remoterev].rev()
    return subset.filter(lambda r: r == rn)

def extsetup(ui):
    templatekw.keywords['gitnode'] = showgitnode
    revset.symbols['gitnode'] = gitnode
