# pager.py - display output using a pager
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# To load the extension, add it to your configuration file:
#
#   [extension]
#   pager =
#
# Run 'hg help pager' to get info on configuration.

'''browse command output with an external pager

To set the pager that should be used, set the application variable::

  [pager]
  pager = less -FRX

If no pager is set, the pager extensions uses the environment variable
$PAGER. If neither pager.pager, nor $PAGER is set, no pager is used.

You can disable the pager for certain commands by adding them to the
pager.ignore list::

  [pager]
  ignore = version, help, update

You can also enable the pager only for certain commands using
pager.attend. Below is the default list of commands to be paged::

  [pager]
  attend = annotate, cat, diff, export, glog, log, qdiff

Setting pager.attend to an empty value will cause all commands to be
paged.

If pager.attend is present, pager.ignore will be ignored.

Lastly, you can enable and disable paging for individual commands with
the attend-<command> option. This setting takes precedence over
existing attend and ignore options and defaults::

  [pager]
  attend-cat = false

To ignore global commands like :hg:`version` or :hg:`help`, you have
to specify them in your user configuration file.

To control whether the pager is used at all for an individual command,
you can use --pager=<value>::

  - use as needed: `auto`.
  - require the pager: `yes` or `on`.
  - suppress the pager: `no` or `off` (any unrecognized value
  will also work).

'''
from __future__ import absolute_import

import atexit
import os
import signal
import subprocess
import sys

from mercurial.i18n import _
from mercurial import (
    cmdutil,
    commands,
    dispatch,
    encoding,
    extensions,
    util,
    )

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

def _runpager(ui, p):
    pager = subprocess.Popen(p, shell=True, bufsize=-1,
                             close_fds=util.closefds, stdin=subprocess.PIPE,
                             stdout=util.stdout, stderr=util.stderr)

    # back up original file descriptors
    stdoutfd = os.dup(util.stdout.fileno())
    stderrfd = os.dup(util.stderr.fileno())

    os.dup2(pager.stdin.fileno(), util.stdout.fileno())
    if ui._isatty(util.stderr):
        os.dup2(pager.stdin.fileno(), util.stderr.fileno())

    @atexit.register
    def killpager():
        if util.safehasattr(signal, "SIGINT"):
            signal.signal(signal.SIGINT, signal.SIG_IGN)
        # restore original fds, closing pager.stdin copies in the process
        os.dup2(stdoutfd, util.stdout.fileno())
        os.dup2(stderrfd, util.stderr.fileno())
        pager.stdin.close()
        pager.wait()

def uisetup(ui):
    class pagerui(ui.__class__):
        def _runpager(self, pagercmd):
            _runpager(self, pagercmd)

    ui.__class__ = pagerui

    def pagecmd(orig, ui, options, cmd, cmdfunc):
        p = ui.config("pager", "pager", encoding.environ.get("PAGER"))
        usepager = False
        always = util.parsebool(options['pager'])
        auto = options['pager'] == 'auto'

        if not p or '--debugger' in sys.argv or not ui.formatted():
            pass
        elif always:
            usepager = True
        elif not auto:
            usepager = False
        else:
            attend = ui.configlist('pager', 'attend', attended)
            ignore = ui.configlist('pager', 'ignore')
            cmds, _ = cmdutil.findcmd(cmd, commands.table)

            for cmd in cmds:
                var = 'attend-%s' % cmd
                if ui.config('pager', var):
                    usepager = ui.configbool('pager', var)
                    break
                if (cmd in attend or
                     (cmd not in ignore and not attend)):
                    usepager = True
                    break

        setattr(ui, 'pageractive', usepager)

        if usepager:
            ui.setconfig('ui', 'formatted', ui.formatted(), 'pager')
            ui.setconfig('ui', 'interactive', False, 'pager')
            ui._runpager(p)
        return orig(ui, options, cmd, cmdfunc)

    # Wrap dispatch._runcommand after color is loaded so color can see
    # ui.pageractive. Otherwise, if we loaded first, color's wrapped
    # dispatch._runcommand would run without having access to ui.pageractive.
    def afterloaded(loaded):
        extensions.wrapfunction(dispatch, '_runcommand', pagecmd)
    extensions.afterloaded('color', afterloaded)

def extsetup(ui):
    commands.globalopts.append(
        ('', 'pager', 'auto',
         _("when to paginate (boolean, always, auto, or never)"),
         _('TYPE')))

attended = ['annotate', 'cat', 'diff', 'export', 'glog', 'log', 'qdiff']
