# gitrevset.py
#
# Copyright 2014 Facebook, Inc.
"""map a git hash to a Mercurial hash:

  $ hg log -r 'gitnode($HASH)'
  $ hg id -r 'gitnode($HASH)'

shortversion:

  $ hg log -r 'g$HASH'
  $ hg id -r 'g$HASH'

"""
from mercurial import extensions
from mercurial import error
from mercurial import hg
from mercurial import templatekw
from mercurial import revset
from mercurial.i18n import _
import re

githashre = re.compile('g([0-9a-fA-F]{40,40})')

def showgitnode(repo, ctx, templ, **args):
    """Return the git revision corresponding to a given hg rev"""
    peerpath = repo.ui.expandpath('default')

    # sshing can cause junk 'remote: ...' output to stdout, so we need to
    # redirect it temporarily so automation can parse the result easily.
    oldfout = repo.ui.fout
    try:
        repo.baseui.fout = repo.ui.ferr
        remoterepo = hg.peer(repo, {}, peerpath)
        remoterev = remoterepo.lookup('_gitlookup_hg_%s' % ctx.hex())
    except error.RepoError:
        # templates are expected to return an empty string when no data exists
        return ''
    finally:
        repo.baseui.fout = oldfout
    return remoterev.encode('hex')

def gitnode(repo, subset, x):
    """``gitnode(id)``
    Return the hg revision corresponding to a given git rev."""
    l = revset.getargs(x, 1, 1, _("id requires one argument"))
    n = revset.getstring(l[0], _("id requires a string"))
    peerpath = repo.ui.expandpath('default')

    # sshing can cause junk 'remote: ...' output to stdout, so we need to
    # redirect it temporarily so automation can parse the result easily.
    oldfout = repo.ui.fout
    try:
        repo.baseui.fout = repo.ui.ferr
        remoterepo = hg.peer(repo, {}, peerpath)
        remoterev = remoterepo.lookup('_gitlookup_git_%s' % n)
    finally:
        repo.baseui.fout = oldfout
    rn = repo[remoterev].rev()
    return subset.filter(lambda r: r == rn)

def overridestringset(orig, repo, subset, x):
    m = githashre.match(x)
    if m is not None:
        return gitnode(repo, subset, ('string', m.group(1)))
    return orig(repo, subset, x)

def extsetup(ui):
    templatekw.keywords['gitnode'] = showgitnode
    revset.symbols['gitnode'] = gitnode
    extensions.wrapfunction(revset, 'stringset', overridestringset)
    revset.symbols['stringset'] = revset.stringset
    revset.methods['string'] = revset.stringset
    revset.methods['symbol'] = revset.stringset
