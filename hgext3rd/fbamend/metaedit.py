# metaedit.py - edit changeset metadata
#
# Copyright 2011 Peter Arrenbrecht <peter.arrenbrecht@gmail.com>
#                Logilab SA        <contact@logilab.fr>
#                Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Patrick Mezard <patrick@mezard.eu>
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    commands,
    error,
    hg,
    lock as lockmod,
    obsolete,
    phases,
    registrar,
    scmutil,
)
from mercurial.i18n import _

from . import (
    common,
    fold,
)

cmdtable = {}
command = registrar.command(cmdtable)

@command('^metaedit',
         [('r', 'rev', [], _("revision to split")),
          ('', 'fold', False, _("fold specified revisions into one")),
         ] + commands.commitopts + commands.commitopts2,
         _('hg metaedit [OPTION]... [-r] [REV]'))
def metaedit(ui, repo, *revs, **opts):
    """edit commit information

    Edits the commit information for the specified revisions. By default, edits
    commit information for the working directory parent.

    With --fold, also folds multiple revisions into one if necessary. In this
    case, the given revisions must form a linear unbroken chain.

    .. container:: verbose

     Some examples:

     - Edit the commit message for the working directory parent::

         hg metaedit

     - Change the username for the working directory parent::

         hg metaedit --user 'New User <new-email@example.com>'

     - Combine all draft revisions that are ancestors of foo but not of @ into
       one::

         hg metaedit --fold 'draft() and only(foo,@)'

       See :hg:`help phases` for more about draft revisions, and
       :hg:`help revsets` for more about the `draft()` and `only()` keywords.
    """
    revs = list(revs)
    revs.extend(opts['rev'])
    if not revs:
        if opts['fold']:
            raise error.Abort(_('revisions must be specified with --fold'))
        revs = ['.']

    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        revs = scmutil.revrange(repo, revs)
        if not opts['fold'] and len(revs) > 1:
            # TODO: handle multiple revisions. This is somewhat tricky because
            # if we want to edit a series of commits:
            #
            #   a ---- b ---- c
            #
            # we need to rewrite a first, then directly rewrite b on top of the
            # new a, then rewrite c on top of the new b. So we need to handle
            # revisions in topological order.
            raise error.Abort(_('editing multiple revisions without --fold is '
                                'not currently supported'))

        if opts['fold']:
            root, head = fold._foldcheck(repo, revs)
        else:
            if repo.revs("%ld and public()", revs):
                raise error.Abort(_('cannot edit commit information for public '
                                    'revisions'))
            root = head = repo[revs.first()]

        wctx = repo[None]
        p1 = wctx.p1()
        tr = repo.transaction('metaedit')
        newp1 = None
        try:
            commitopts = opts.copy()
            allctx = [repo[r] for r in revs]
            targetphase = max(c.phase() for c in allctx)

            if commitopts.get('message') or commitopts.get('logfile'):
                commitopts['edit'] = False
            else:
                if opts['fold']:
                    msgs = [_("HG: This is a fold of %d changesets.")
                            % len(allctx)]
                    msgs += [_("HG: Commit message of changeset %s.\n\n%s\n")
                             % (c.rev(), c.description()) for c in allctx]
                else:
                    msgs = [head.description()]
                commitopts['message'] = "\n".join(msgs)
                commitopts['edit'] = True

            # TODO: if the author and message are the same, don't create a new
            # hash. Right now we create a new hash because the date can be
            # different.
            newid, created = common.rewrite(
                repo, root, allctx, head, [root.p1().node(), root.p2().node()],
                commitopts=commitopts)
            if created:
                if p1.rev() in revs:
                    newp1 = newid
                phases.retractboundary(repo, tr, targetphase, [newid])
                obsolete.createmarkers(repo, [(ctx, (repo[newid],))
                                              for ctx in allctx])
            else:
                ui.status(_("nothing changed\n"))
                return 1
            tr.close()
        finally:
            tr.release()

        if opts['fold']:
            ui.status(_('%i changesets folded\n') % len(revs))
        if newp1 is not None:
            hg.update(repo, newp1)
    finally:
        lockmod.release(lock, wlock)
