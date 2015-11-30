# fbhistedit.py - improved amend functionality
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""extends the existing histedit functionality

Adds a s/stop verb to histedit to stop after a commit was picked.
"""

import os
from pipes import quote

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

def defineactions():
    histedit = extensions.find('histedit')
    class stop(histedit.histeditaction):
        def run(self):
            parentctx, replacements = super(stop, self).run()
            self.state.read()
            self.state.replacements.extend(replacements)
            self.state.write()
            raise error.InterventionRequired(
                _('Changes commited as %s. You may amend the commit now.\n'
                  'When you are finished, run hg histedit --continue to resume') %
                parentctx)

        def continueclean(self):
            self.state.replacements = [(n, r) for (n, r) \
                                       in self.state.replacements \
                                       if n!=self.node]
            return super(stop, self).continueclean()

    class execute(histedit.histeditaction):
        def __init__(self, state, command):
            self.state = state
            self.repo = state.repo
            self.command = command
            self.cwd = state.repo.root

        @classmethod
        def fromrule(cls, state, rule):
            """Parses the given rule, returning an instance of the histeditaction.
            """
            command = rule
            return cls(state, command)

        def constraints(self):
            return set()

        def nodetoverify(self):
            return None

        def run(self):
            state = self.state
            repo, ctxnode = state.repo, state.parentctxnode
            hg.update(repo, ctxnode)

            # release locks so the programm can call hg and then relock.
            lock.release(state.lock, state.wlock)

            try:
                ctx = repo[ctxnode]
                shell = os.environ.get('SHELL', None)
                cmd = self.command
                if shell and self.repo.ui.config('fbhistedit', 'exec_in_user_shell'):
                    cmd = "%s -c -i %s" % (shell, quote(cmd))
                rc = util.system(cmd,  environ={'HGNODE': ctx.hex()},
                                    cwd=self.cwd)
            except OSError as ose:
                raise error.InterventionRequired(
                    _("Cannot execute command '%s': %s") % (self.command, ose))
            finally:
                # relock the repository
                state.wlock = repo.wlock()
                state.lock = repo.lock()
                repo.invalidate()
                repo.invalidatedirstate()

            if rc != 0:
                raise error.InterventionRequired(
                    _("Command '%s' failed with exit status %d") %
                    (self.command, rc))

            m, a, r, d = self.repo.status()[:4]
            if m or a or r or d:
                self.continuedirty()
            return self.continueclean()

        def continuedirty(self):
            raise util.Abort(_('working copy has pending changes'),
                hint=_('amend, commit, or revert them and run histedit '
                    '--continue, or abort with histedit --abort'))

        def continueclean(self):
            parentctxnode = self.state.parentctxnode
            newctx = self.repo['.']
            if newctx.node() != parentctxnode:
                return newctx, [(parentctxnode, (newctx.node(),))]
            return newctx, []

    class executerelative(execute):
        def __init__(self, state, command):
            super(executerelative, self).__init__(state, command)
            self.cwd = os.getcwd()

    return stop, execute, executerelative

def extsetup(ui):
    histedit = extensions.find('histedit')
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
#  xr, execr = execute given command relative to current directory
#
    """)
    stop, execute, executerel = defineactions()
    histedit.actiontable['s'] = stop
    histedit.actiontable['stop'] = stop
    histedit.actiontable['x'] = execute
    histedit.actiontable['exec'] = execute
    histedit.actiontable['xr'] = executerel
    histedit.actiontable['execr'] = executerel
