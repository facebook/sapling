# Copyright (C) 2007 Brendan Cully <brendan@kublai.com>
# This file is published under the GNU GPL.

'''allow user-defined command aliases

To use, create entries in your hgrc of the form

[alias]
mycmd = cmd --args
'''

from mercurial.cmdutil import findcmd, UnknownCommand, AmbiguousCommand
from mercurial import commands

cmdtable = {}

class RecursiveCommand(Exception): pass

class lazycommand(object):
    '''defer command lookup until needed, so that extensions loaded
    after alias can be aliased'''
    def __init__(self, ui, name, target):
        self._ui = ui
        self._name = name
        self._target = target
        self._cmd = None

    def __len__(self):
        self._resolve()
        return len(self._cmd)

    def __getitem__(self, key):
        self._resolve()
        return self._cmd[key]

    def __iter__(self):
        self._resolve()
        return self._cmd.__iter__()

    def _resolve(self):
        if self._cmd is not None:
            return

        try:
            self._cmd = findcmd(self._ui, self._target, commands.table)[1]
            if self._cmd == self:
                raise RecursiveCommand()
            if self._target in commands.norepo.split(' '):
                commands.norepo += ' %s' % self._name
            return
        except UnknownCommand:
            msg = '*** [alias] %s: command %s is unknown' % \
                  (self._name, self._target)
        except AmbiguousCommand:
            msg = '*** [alias] %s: command %s is ambiguous' % \
                  (self._name, self._target)
        except RecursiveCommand:
            msg = '*** [alias] %s: circular dependency on %s' % \
                  (self._name, self._target)
        def nocmd(*args, **opts):
            self._ui.warn(msg + '\n')
            return 1
        nocmd.__doc__ = msg
        self._cmd = (nocmd, [], '')
        commands.norepo += ' %s' % self._name

def uisetup(ui):
    for cmd, target in ui.configitems('alias'):
        if not target:
            ui.warn('*** [alias] %s: no definition\n' % cmd)
            continue
        args = target.split(' ')
        tcmd = args.pop(0)
        if args:
            ui.setconfig('defaults', cmd, ' '.join(args))
        cmdtable[cmd] = lazycommand(ui, cmd, tcmd)
