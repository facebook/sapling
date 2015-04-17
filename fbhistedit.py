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
from mercurial import extensions
from mercurial import hg
from mercurial import lock
from mercurial import util
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

testedwith = 'internal'

class stop(histedit.histeditaction):
    def run(self):
        parentctx, replacements = super(stop, self).run()
        raise error.InterventionRequired(
            _('Changes commited as %s. You may amend the commit now.\n'
              'When you are finished, run hg histedit --continue to resume') %
            parentctx)

def execute(ui, state, cmd, opts):
    repo, ctxnode = state.repo, state.parentctxnode
    hg.update(repo, ctxnode)

    # release locks so the programm can call hg and then relock.
    lock.release(state.lock, state.wlock)

    try:
        ctx = repo[ctxnode]
        rc = util.system(cmd, environ={'HGNODE': ctx.hex()}, cwd=repo.root)
    except OSError as os:
        raise error.InterventionRequired(
            _("Cannot execute command '%s': %s") % (cmd, os))
    finally:
        # relock the repository
        state.wlock = repo.wlock()
        state.lock = repo.lock()
        repo.invalidate()
        repo.invalidatedirstate()

    if rc != 0:
        raise error.InterventionRequired(
            _("Command '%s' failed with exit status %d") % (cmd, rc))

    if util.any(repo.status()[:4]):
        raise error.InterventionRequired(
            _('Fix up the change and run hg histedit --continue'))

    newctx = repo['.']
    if ctxnode != newctx.node():
        return newctx, [(ctxnode, (newctx.node(),))]
    return newctx, []

# HACK:
# The following function verifyrules and bootstrap continue are copied from
# histedit.py as we have no proper way of fixing up the x/exec specialcase.
def verifyrules(orig, rules, repo, ctxs):
    """Verify that there exists exactly one edit rule per given changeset.

    Will abort if there are to many or too few rules, a malformed rule,
    or a rule on a changeset outside of the user-given range.
    """
    parsed = []
    expected = set(c.hex() for c in ctxs)
    seen = set()
    for r in rules:
        if ' ' not in r:
            raise util.Abort(_('malformed line "%s"') % r)
        action, rest = r.split(' ', 1)
        # Our x/exec specialcasing
        if action in ['x', 'exec']:
            parsed.append([action, rest])
        else:
            ha = rest.strip().split(' ', 1)[0]
            try:
                ha = repo[ha].hex()
            except error.RepoError:
                raise util.Abort(_('unknown changeset %s listed') % ha[:12])
            if ha not in expected:
                raise util.Abort(
                    _('may not use changesets other than the ones listed'))
            if ha in seen:
                raise util.Abort(_('duplicated command for changeset %s') %
                        ha[:12])
            seen.add(ha)
            if action not in histedit.actiontable:
                raise util.Abort(_('unknown action "%s"') % action)
            parsed.append([action, ha])
    missing = sorted(expected - seen)  # sort to stabilize output
    if missing:
        raise util.Abort(_('missing rules for changeset %s') % missing[0],
                         hint=_('do you want to use the drop action?'))
    return parsed

def bootstrapcontinue(orig, ui, state, opts):
    repo, parentctxnode = state.repo, state.parentctxnode
    if state.rules[0][0] in ['x', 'exec']:
        m, a, r, d = repo.status()[:4]
        if m or a or r or d:
            raise util.Abort(_('working copy has pending changes'),
                hint=_('amend, commit, or revert them and run histedit '
                    '--continue, or abort with histedit --abort'))

        state.rules.pop(0)
        state.parentctxnode = parentctxnode
        return state
    else:
        return orig(ui, state, opts)

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
#  x, exec = execute given command
#
    """)
    histedit.actiontable['s'] = stop
    histedit.actiontable['stop'] = stop
    histedit.actiontable['x'] = execute
    histedit.actiontable['exec'] = execute

    extensions.wrapfunction(histedit, 'bootstrapcontinue', bootstrapcontinue)
    extensions.wrapfunction(histedit, 'verifyrules', verifyrules)
