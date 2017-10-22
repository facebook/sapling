# fetch.py - pull and merge remote changes
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''pull, update and merge in one command (DEPRECATED)'''

from __future__ import absolute_import

from mercurial.i18n import _
from mercurial.node import (
    short,
)
from mercurial import (
    cmdutil,
    error,
    exchange,
    hg,
    lock,
    pycompat,
    registrar,
    util,
)

release = lock.release
cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

@command('fetch',
    [('r', 'rev', [],
     _('a specific revision you would like to pull'), _('REV')),
    ('', 'edit', None, _('invoke editor on commit messages')),
    ('', 'force-editor', None, _('edit commit message (DEPRECATED)')),
    ('', 'switch-parent', None, _('switch parents when merging')),
    ] + cmdutil.commitopts + cmdutil.commitopts2 + cmdutil.remoteopts,
    _('hg fetch [SOURCE]'))
def fetch(ui, repo, source='default', **opts):
    '''pull changes from a remote repository, merge new changes if needed.

    This finds all changes from the repository at the specified path
    or URL and adds them to the local repository.

    If the pulled changes add a new branch head, the head is
    automatically merged, and the result of the merge is committed.
    Otherwise, the working directory is updated to include the new
    changes.

    When a merge is needed, the working directory is first updated to
    the newly pulled changes. Local changes are then merged into the
    pulled changes. To switch the merge order, use --switch-parent.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Returns 0 on success.
    '''

    opts = pycompat.byteskwargs(opts)
    date = opts.get('date')
    if date:
        opts['date'] = util.parsedate(date)

    parent, _p2 = repo.dirstate.parents()
    branch = repo.dirstate.branch()
    try:
        branchnode = repo.branchtip(branch)
    except error.RepoLookupError:
        branchnode = None
    if parent != branchnode:
        raise error.Abort(_('working directory not at branch tip'),
                         hint=_("use 'hg update' to check out branch tip"))

    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        cmdutil.bailifchanged(repo)

        bheads = repo.branchheads(branch)
        bheads = [head for head in bheads if len(repo[head].children()) == 0]
        if len(bheads) > 1:
            raise error.Abort(_('multiple heads in this branch '
                               '(use "hg heads ." and "hg merge" to merge)'))

        other = hg.peer(repo, opts, ui.expandpath(source))
        ui.status(_('pulling from %s\n') %
                  util.hidepassword(ui.expandpath(source)))
        revs = None
        if opts['rev']:
            try:
                revs = [other.lookup(rev) for rev in opts['rev']]
            except error.CapabilityError:
                err = _("other repository doesn't support revision lookup, "
                        "so a rev cannot be specified.")
                raise error.Abort(err)

        # Are there any changes at all?
        modheads = exchange.pull(repo, other, heads=revs).cgresult
        if modheads == 0:
            return 0

        # Is this a simple fast-forward along the current branch?
        newheads = repo.branchheads(branch)
        newchildren = repo.changelog.nodesbetween([parent], newheads)[2]
        if len(newheads) == 1 and len(newchildren):
            if newchildren[0] != parent:
                return hg.update(repo, newchildren[0])
            else:
                return 0

        # Are there more than one additional branch heads?
        newchildren = [n for n in newchildren if n != parent]
        newparent = parent
        if newchildren:
            newparent = newchildren[0]
            hg.clean(repo, newparent)
        newheads = [n for n in newheads if n != newparent]
        if len(newheads) > 1:
            ui.status(_('not merging with %d other new branch heads '
                        '(use "hg heads ." and "hg merge" to merge them)\n') %
                      (len(newheads) - 1))
            return 1

        if not newheads:
            return 0

        # Otherwise, let's merge.
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
            # we don't translate commit messages
            message = (cmdutil.logmessage(ui, opts) or
                       ('Automated merge with %s' %
                        util.removeauth(other.url())))
            editopt = opts.get('edit') or opts.get('force_editor')
            editor = cmdutil.getcommiteditor(edit=editopt, editform='fetch')
            n = repo.commit(message, opts['user'], opts['date'], editor=editor)
            ui.status(_('new changeset %d:%s merges remote changes '
                        'with local\n') % (repo.changelog.rev(n),
                                           short(n)))

        return err

    finally:
        release(lock, wlock)
