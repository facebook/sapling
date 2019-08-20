# nointerrupt.py - prevent mercurial from being ctrl-c'ed
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""warns but doesn't exit when the user first hits Ctrl+C

This extension shows a warning the first time a user hits Ctrl+C saying the
repository could end up in a bad state. If the user hits Ctrl+C a second time,
hg will exit as usual.

By default, this behavior only applies to commands that are explicitly
whitelisted for it. To whitelist a command (say, "commit"), use:

  [nointerrupt]
  attend-commit = true

To change the default behavior to have all commands instrumented, set config
option ``nointerrupt.default-attend`` to true, then use the same logic to
disable it for commands where it's not wanted - for instance, for "log":

  [nointerrupt]
  default-attend = true
  attend-log = false

Finally, to customize the message shown on the first Ctrl+C, set it in
config option ``nointerrupt.message``.
"""

import signal
import sys

from edenscm.mercurial import cmdutil, commands, dispatch, extensions


def sigintprintwarninghandlerfactory(oldsiginthandler, msg):
    def sigint(*args):
        sys.stderr.write(msg)
        signal.signal(signal.SIGINT, oldsiginthandler)

    return sigint


def nointerruptcmd(orig, ui, options, cmd, cmdfunc):
    # bail if not in interactive terminal
    if ui.configbool("nointerrupt", "interactiveonly", True):
        if not ui.fout.isatty() or ui.plain():
            return orig(ui, options, cmd, cmdfunc)

    cmds, _cmdtableentry = cmdutil.findcmd(cmd, commands.table)

    shouldpreventinterrupt = ui.configbool("nointerrupt", "default-attend", False)
    for cmd in cmds:
        var = "attend-%s" % cmd
        if ui.config("nointerrupt", var):
            shouldpreventinterrupt = ui.configbool("nointerrupt", var)
            break

    if shouldpreventinterrupt:
        oldsiginthandler = signal.getsignal(signal.SIGINT)
        try:
            msg = ui.config(
                "nointerrupt",
                "message",
                "==========================\n"
                "Interrupting Mercurial may leave your repo in a bad state.\n"
                "If you really want to interrupt your current command, press\n"
                "CTRL-C again.\n"
                "==========================\n",
            )
            signal.signal(
                signal.SIGINT, sigintprintwarninghandlerfactory(oldsiginthandler, msg)
            )
        except AttributeError:
            pass
    return orig(ui, options, cmd, cmdfunc)


def uisetup(ui):
    extensions.wrapfunction(dispatch, "_runcommand", nointerruptcmd)
