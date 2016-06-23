# nointerrupt.py - prevent mercurial from being ctrl-c'ed
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import sys, signal

from mercurial import cmdutil, commands, dispatch, extensions

def sigintprintwarninghandlerfactory(oldsiginthandler, msg):
    def sigint(*args):
        sys.stderr.write(msg)
        signal.signal(signal.SIGINT, oldsiginthandler)
    return sigint

def nointerruptcmd(orig, ui, options, cmd, cmdfunc):
    # bail if not in interactive terminal
    if ui.configbool('nointerrupt', 'interactiveonly', True):
        if not ui.fout.isatty() or ui.plain():
            return orig(ui, options, cmd, cmdfunc)

    cmds, _cmdtableentry = cmdutil.findcmd(cmd, commands.table)
    if isinstance(_cmdtableentry[0], dispatch.cmdalias):
        cmds.append(_cmdtableentry[0].cmdname)

    shouldpreventinterrupt = False
    for cmd in cmds:
        var = 'attend-%s' % cmd
        if ui.config('nointerrupt', var):
            shouldpreventinterrupt = ui.configbool('nointerrupt', var)
            break

    if shouldpreventinterrupt:
        oldsiginthandler = signal.getsignal(signal.SIGINT)
        try:
            msg = ui.config('nointerrupt', 'message',
                "==========================\n"
                "Interrupting Mercurial may leave your repo in a bad state.\n"
                "If you really want to interrupt your current command, press\n"
                "CTRL-C again.\n"
                "==========================\n"
            )
            signal.signal(signal.SIGINT, sigintprintwarninghandlerfactory(
                oldsiginthandler, msg))
        except AttributeError:
            pass
    return orig(ui, options, cmd, cmdfunc)

def uisetup(ui):
    extensions.wrapfunction(dispatch, '_runcommand', nointerruptcmd)
