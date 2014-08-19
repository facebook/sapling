# tweakdefaults.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import util, cmdutil, commands, hg
from mercurial import bookmarks
from mercurial.extensions import wrapcommand
from mercurial.i18n import _
from hgext import rebase
import errno, os

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def update(orig, ui, repo, node=None, rev=None, **kwargs):
    # 'hg update' should do nothing
    if not node and not rev:
        raise util.Abort("you must specify a destination to update to " +
            "(if you're trying to move a bookmark forward, try " +
            "'hg rebase -d <destination>')")

    return orig(ui, repo, node=node, **kwargs)
wrapcommand(commands.table, 'update', update)

def _rebase(orig, ui, repo, **opts):
    if opts.get('continue') or opts.get('abort'):
        return orig(ui, repo, **opts)

    # 'hg rebase' w/o args should do nothing
    if not opts.get('dest'):
        raise util.Abort("you must specify a destination (-d) for the rebase")

    # 'hg rebase' can fast forwards bookmark
    cl = repo.changelog
    prev = repo.revs('.')
    dest = repo.revs(opts.get('dest'))

    # Only fastward the bookmark if there is a single destination rev, and if
    # no source nodes were explicitly specified.
    if (prev and dest and len(dest) == 1 and not opts.get('base') and
       not opts.get('source') and not opts.get('rev')):
        prev = cl.node(prev[0])
        dest = cl.node(dest[0])
        common = cl.ancestor(prev, dest)
        if prev == common:
            result = hg.update(repo, dest)
            if repo._bookmarkcurrent:
                bookmarks.update(repo, [prev], dest)
            return result

    return orig(ui, repo, **opts)
wrapcommand(rebase.cmdtable, 'rebase', _rebase)

logopts = [
    ('', 'all', None, _('shows all commits in the repo')),
]

def log(orig, ui, repo, *pats, **opts):
    # 'hg log' defaults to -f
    # All special uses of log (--date, --branch, etc) will also now do follow.
    if not opts.get('rev') and not opts.get('all'):
        opts['follow'] = True

    return orig(ui, repo, *pats, **opts)

entry = wrapcommand(commands.table, 'log', log)
for opt in logopts:
    opt = (opt[0], opt[1], opt[2], opt[3])
    entry[1].append(opt)
