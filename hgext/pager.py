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
# Run "hg help pager" to get info on configuration.

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

from mercurial import (
    cmdutil,
    commands,
    dispatch,
    extensions,
    util,
    )
from mercurial.i18n import _

# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

def _runpager(ui, p):
    pager = subprocess.Popen(p, shell=True, bufsize=-1,
                             close_fds=util.closefds, stdin=subprocess.PIPE,
                             stdout=sys.stdout, stderr=sys.stderr)

    # back up original file objects and descriptors
    olduifout = ui.fout
    oldstdout = sys.stdout
    stdoutfd = os.dup(sys.stdout.fileno())
    stderrfd = os.dup(sys.stderr.fileno())

    # create new line-buffered stdout so that output can show up immediately
    ui.fout = sys.stdout = newstdout = os.fdopen(sys.stdout.fileno(), 'wb', 1)
    os.dup2(pager.stdin.fileno(), sys.stdout.fileno())
    if ui._isatty(sys.stderr):
        os.dup2(pager.stdin.fileno(), sys.stderr.fileno())

    @atexit.register
    def killpager():
        if util.safehasattr(signal, "SIGINT"):
            signal.signal(signal.SIGINT, signal.SIG_IGN)
        pager.stdin.close()
        ui.fout = olduifout
        sys.stdout = oldstdout
        # close new stdout while it's associated with pager; otherwise stdout
        # fd would be closed when newstdout is deleted
        newstdout.close()
        # restore original fds: stdout is open again
        os.dup2(stdoutfd, sys.stdout.fileno())
        os.dup2(stderrfd, sys.stderr.fileno())
        pager.wait()

def uisetup(ui):
    if '--debugger' in sys.argv or not ui.formatted():
        return

    # chg has its own pager implementation
    argv = sys.argv[:]
    if 'chgunix' in dispatch._earlygetopt(['--cmdserver'], argv):
        return

    def pagecmd(orig, ui, options, cmd, cmdfunc):
        p = ui.config("pager", "pager", os.environ.get("PAGER"))
        usepager = False
        always = util.parsebool(options['pager'])
        auto = options['pager'] == 'auto'

        if not p:
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
            if util.safehasattr(signal, "SIGPIPE"):
                signal.signal(signal.SIGPIPE, signal.SIG_DFL)
            _runpager(ui, p)
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
