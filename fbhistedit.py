# fbhistedit.py - improved amend functionality
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""extends the existing histedit functionality

Adds a s/stop verb to histedit to stop after a commit was picked.
"""

from hgext import histedit
from mercurial import cmdutil
from mercurial import error
from mercurial import hg
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

testedwith = 'internal'

def stop(ui, repo, ctx, ha, opts):
    oldctx = repo[ha]

    hg.update(repo, ctx.node())
    stats = histedit.applychanges(ui, repo, oldctx, opts)
    if stats and stats[3] > 0:
        raise error.InterventionRequired(
            _('Fix up the change and run hg histedit --continue'))

    commit = histedit.commitfuncfor(repo, oldctx)
    new = commit(text=oldctx.description(), user=oldctx.user(),
            date=oldctx.date(), extra=oldctx.extra())

    raise error.InterventionRequired(
        _('Changes commited as %s. You may amend the commit now.\n'
          'When you are finished, run hg histedit --continue to resume.') %
        repo[new])

def extsetup(ui):
    histedit.editcomment = _("""# Edit history between %s and %s
#
# Commits are listed from least to most recent
#
# Commands:
#  p, pick = use commit
#  e, edit = use commit, but stop for amending
#  s, stop = use commit, and stop after committing changes
#  f, fold = use commit, but combine it with the one above
#  r, roll = like fold, but discard this commit's description
#  d, drop = remove commit from history
#  m, mess = edit message without changing commit content
#
    """)
    histedit.actiontable['s'] = stop
    histedit.actiontable['stop'] = stop
