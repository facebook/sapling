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
from mercurial import node
from mercurial import scmutil
from mercurial import util
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

testedwith = 'internal'

def defineactions():
    histedit = extensions.find('histedit')
    @histedit.action(['stop', 's'],
                     _('pick commit, and stop after committing changes'))
    class stop(histedit.histeditaction):
        def run(self):
            parentctx, replacements = super(stop, self).run()
            self.state.read()
            self.state.replacements.extend(replacements)
            self.state.write()
            raise error.InterventionRequired(
                _('Changes commited as %s. You may amend the commit now.\n'
                  'When you are done, run hg histedit --continue to resume') %
                parentctx)

        def continueclean(self):
            self.state.replacements = [(n, r) for (n, r) \
                                       in self.state.replacements \
                                       if n != self.node]
            return super(stop, self).continueclean()

    @histedit.action(['exec', 'x'],
                     _('execute given command'))
    class execute(histedit.histeditaction):
        def __init__(self, state, command):
            self.state = state
            self.repo = state.repo
            self.command = command
            self.cwd = state.repo.root
            self.node = None

        @classmethod
        def fromrule(cls, state, rule):
            """Parses the given rule, returns an instance of the histeditaction.
            """
            command = rule
            return cls(state, command)

        def torule(self, *args, **kwargs):
            return "%s %s" % (self.verb, self.command)

        def tostate(self):
            """Print an action in format used by histedit state files
            (the first line is a verb, the remainder is the second)
            """
            return "%s\n%s" % (self.verb, self.command)

        def verify(self, *args, **kwds):
            pass

        def constraints(self):
            return set()

        def nodetoverify(self):
            return None

        def run(self):
            state = self.state
            repo, ctxnode = state.repo, state.parentctxnode
            hg.update(repo, ctxnode)

            # release locks so the program can call hg and then relock.
            lock.release(state.lock, state.wlock)

            try:
                ctx = repo[ctxnode]
                shell = os.environ.get('SHELL', None)
                cmd = self.command
                if shell and self.repo.ui.config('fbhistedit',
                                                 'exec_in_user_shell'):
                    cmd = "%s -c -i %s" % (shell, quote(cmd))
                rc = repo.ui.system(cmd,  environ={'HGNODE': ctx.hex()},
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
            raise error.Abort(_('working copy has pending changes'),
                hint=_('amend, commit, or revert them and run histedit '
                    '--continue, or abort with histedit --abort'))

        def continueclean(self):
            parentctxnode = self.state.parentctxnode
            newctx = self.repo['.']
            if newctx.node() != parentctxnode:
                return newctx, [(parentctxnode, (newctx.node(),))]
            return newctx, []

    @histedit.action(['execr', 'xr'],
                     _('execute given command relative to current directory'))
    class executerelative(execute):
        def __init__(self, state, command):
            super(executerelative, self).__init__(state, command)
            self.cwd = os.getcwd()

    return stop, execute, executerelative

def extsetup(ui):
    histedit = extensions.find('histedit')
    stop, execute, executerel = defineactions()

    if ui.config('experimental', 'histeditng'):
        rebase = extensions.find('rebase')
        extensions.wrapcommand(rebase.cmdtable, 'rebase', _rebase,
                               synopsis=' [-i]')

        aliases, entry = cmdutil.findcmd('rebase', rebase.cmdtable)
        newentry = list(entry)
        options = newentry[1]
        # dirty hack because we need to change an existing switch
        for idx, opt in enumerate(options):
            if opt[0] == 'i':
                del options[idx]
        options.append(('i', 'interactive', False, 'interactive rebase'))
        rebase.cmdtable['rebase'] = tuple(newentry)


def _rebase(orig, ui, repo, **opts):
    histedit = extensions.find('histedit')

    contf = opts.get('continue')
    abortf = opts.get('abort')

    if (contf or abortf) and \
            not repo.vfs.exists('rebasestate') and\
            repo.vfs.exists('histedit.state'):
        msg = _("no rebase in progress")
        hint = _('If you want to continue or abort an interactive rebase please'
                 ' use "histedit --continue/--abort" instead.')
        raise error.Abort(msg, hint=hint)

    if not opts.get('interactive'):
        return orig(ui, repo, **opts)


    # the argument parsing has as lot of copy-paste from rebase.py
    # Validate input and define rebasing points
    destf = opts.get('dest', None)
    srcf = opts.get('source', None)
    basef = opts.get('base', None)
    revf = opts.get('rev', [])
    keepf = opts.get('keep', False)

    src = None

    if contf or abortf:
        raise error.Abort('no interactive rebase in progress')
    if destf:
        dest = scmutil.revsingle(repo, destf)
    else:
        raise error.Abort("you must specify a destination (-d) for the rebase")

    if srcf and basef:
        raise error.Abort(_('cannot specify both a source and a base'))
    if revf:
        raise error.Abort('--rev not supported with interactive rebase')
    elif srcf:
        src = scmutil.revsingle(repo, srcf)
    else:
        base = scmutil.revrange(repo, [basef or '.'])
        if not base:
            ui.status(_('empty "base" revision set - '
                        "can't compute rebase set\n"))
            return 1
        commonanc = repo.revs('ancestor(%ld, %d)', base, dest).first()
        if commonanc is not None:
            src = repo.revs('min((%d::(%ld) - %d)::)',
                            commonanc, base, commonanc).first()
        else:
            src = None

    if src is None:
        raise error.Abort('no revisions to rebase')
    src = repo[src].node()

    topmost, empty = repo.dirstate.parents()
    revs = histedit.between(repo, src, topmost, keepf)
    ctxs = [repo[r] for r in revs]
    state = histedit.histeditstate(repo)
    rules = [histedit.base(state, repo[dest])] + \
        [histedit.pick(state, ctx) for ctx in ctxs]
    editcomment = """#
# Interactive rebase is just a wrapper over histedit (adding the 'base' line as
# the first rule). To continue or abort it you should use:
# "hg histedit --continue" and "--abort"
#
"""
    editcomment += histedit.geteditcomment(ui, node.short(src),
                                           node.short(topmost))
    histedit.ruleeditor(repo, ui, rules, editcomment=editcomment)

    return histedit.histedit(ui, repo, node.hex(src), keep=keepf,
                             commands=repo.join('histedit-last-edit.txt'))
