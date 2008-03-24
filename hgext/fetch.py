# fetch.py - pull and merge remote changes
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import _
from mercurial.node import nullid, short
from mercurial import commands, cmdutil, hg, util

def fetch(ui, repo, source='default', **opts):
    '''Pull changes from a remote repository, merge new changes if needed.

    This finds all changes from the repository at the specified path
    or URL and adds them to the local repository.

    If the pulled changes add a new head, the head is automatically
    merged, and the result of the merge is committed.  Otherwise, the
    working directory is updated to include the new changes.

    When a merge occurs, the newly pulled changes are assumed to be
    "authoritative".  The head of the new changes is used as the first
    parent, with local changes as the second.  To switch the merge
    order, use --switch-parent.

    See 'hg help dates' for a list of formats valid for -d/--date.
    '''

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
        if len(newheads) > 1:
            ui.status(_('not merging with %d other new heads '
                        '(use "hg heads" and "hg merge" to merge them)') %
                      (len(newheads) - 1))
            return
        err = False
        if newheads:
            # By default, we consider the repository we're pulling
            # *from* as authoritative, so we merge our changes into
            # theirs.
            if opts['switch_parent']:
                firstparent, secondparent = newparent, newheads[0]
            else:
                firstparent, secondparent = newheads[0], newparent
                ui.status(_('updating to %d:%s\n') %
                          (repo.changelog.rev(firstparent),
                           short(firstparent)))
            hg.clean(repo, firstparent)
            ui.status(_('merging with %d:%s\n') %
                      (repo.changelog.rev(secondparent), short(secondparent)))
            err = hg.merge(repo, secondparent, remind=False)
        if not err:
            mod, add, rem = repo.status()[:3]
            message = (cmdutil.logmessage(opts) or
                       (_('Automated merge with %s') %
                        util.removeauth(other.url())))
            force_editor = opts.get('force_editor') or opts.get('edit')
            n = repo.commit(mod + add + rem, message,
                            opts['user'], opts['date'], force=True,
                            force_editor=force_editor)
            ui.status(_('new changeset %d:%s merges remote changes '
                        'with local\n') % (repo.changelog.rev(n),
                                           short(n)))

    def pull():
        cmdutil.setremoteconfig(ui, opts)

        other = hg.repository(ui, ui.expandpath(source))
        ui.status(_('pulling from %s\n') %
                  util.hidepassword(ui.expandpath(source)))
        revs = None
        if opts['rev']:
            if not other.local():
                raise util.Abort(_("fetch -r doesn't work for remote "
                                   "repositories yet"))
            else:
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
        mod, add, rem, del_ = repo.status()[:4]
        if mod or add or rem:
            raise util.Abort(_('outstanding uncommitted changes'))
        if del_:
            raise util.Abort(_('working directory is missing some files'))
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
         ('e', 'edit', None, _('edit commit message')),
         ('', 'force-editor', None, _('edit commit message (DEPRECATED)')),
         ('', 'switch-parent', None, _('switch parents when merging')),
        ] + commands.commitopts + commands.commitopts2 + commands.remoteopts,
        _('hg fetch [SOURCE]')),
}
