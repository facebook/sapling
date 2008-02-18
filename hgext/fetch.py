# fetch.py - pull and merge remote changes
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import _
from mercurial.node import *
from mercurial import commands, cmdutil, hg, node, util

def fetch(ui, repo, source='default', **opts):
    '''Pull changes from a remote repository, merge new changes if needed.

    This finds all changes from the repository at the specified path
    or URL and adds them to the local repository.

    If the pulled changes add a new head, the head is automatically
    merged, and the result of the merge is committed.  Otherwise, the
    working directory is updated.'''

    def postincoming(other, modheads):
        if modheads == 0:
            return 0
        if modheads == 1:
            return hg.clean(repo, repo.changelog.tip())
        newheads = repo.heads(parent)
        newchildren = [n for n in repo.heads(parent) if n != parent]
        newparent = parent
        if newchildren:
            newparent = newchildren[0]
            hg.clean(repo, newparent)
        newheads = [n for n in repo.heads() if n != newparent]
        err = False
        if newheads:
            ui.status(_('merging with new head %d:%s\n') %
                      (repo.changelog.rev(newheads[0]), short(newheads[0])))
            err = hg.merge(repo, newheads[0], remind=False)
        if not err and len(newheads) > 1:
            ui.status(_('not merging with %d other new heads '
                        '(use "hg heads" and "hg merge" to merge them)') %
                      (len(newheads) - 1))
        if not err:
            mod, add, rem = repo.status()[:3]
            message = (cmdutil.logmessage(opts) or
                       (_('Automated merge with %s') %
                        util.removeauth(other.url())))
            n = repo.commit(mod + add + rem, message,
                            opts['user'], opts['date'],
                            force_editor=opts.get('force_editor'))
            ui.status(_('new changeset %d:%s merges remote changes '
                        'with local\n') % (repo.changelog.rev(n),
                                           short(n)))
    def pull():
        cmdutil.setremoteconfig(ui, opts)

        other = hg.repository(ui, ui.expandpath(source))
        ui.status(_('pulling from %s\n') %
                  util.hidepassword(ui.expandpath(source)))
        revs = None
        if opts['rev'] and not other.local():
            raise util.Abort(_("fetch -r doesn't work for remote repositories yet"))
        elif opts['rev']:
            revs = [other.lookup(rev) for rev in opts['rev']]
        modheads = repo.pull(other, heads=revs)
        return postincoming(other, modheads)

    date = opts.get('date')
    if date:
        opts['date'] = util.parsedate(date)

    parent, p2 = repo.dirstate.parents()
    if parent != repo.changelog.tip():
        raise util.Abort(_('working dir not at tip '
                           '(use "hg update" to check out tip)'))
    if p2 != nullid:
        raise util.Abort(_('outstanding uncommitted merge'))
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        mod, add, rem = repo.status()[:3]
        if mod or add or rem:
            raise util.Abort(_('outstanding uncommitted changes'))
        if len(repo.heads()) > 1:
            raise util.Abort(_('multiple heads in this repository '
                               '(use "hg heads" and "hg merge" to merge)'))
        return pull()
    finally:
        del lock, wlock

cmdtable = {
    'fetch':
        (fetch,
        [('r', 'rev', [], _('a specific revision you would like to pull')),
         ('f', 'force-editor', None, _('edit commit message')),
        ] + commands.commitopts + commands.commitopts2 + commands.remoteopts,
        _('hg fetch [SOURCE]')),
}
