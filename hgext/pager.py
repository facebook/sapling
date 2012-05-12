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
  pager = less -FRSX

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

To ignore global commands like :hg:`version` or :hg:`help`, you have
to specify them in your user configuration file.

The --pager=... option can also be used to control when the pager is
used. Use a boolean value like yes, no, on, off, or use auto for
normal behavior.
'''

import atexit, sys, os, signal, subprocess
from mercurial import commands, dispatch, util, extensions
from mercurial.i18n import _

def _runpager(p):
    pager = subprocess.Popen(p, shell=True, bufsize=-1,
                             close_fds=util.closefds, stdin=subprocess.PIPE,
                             stdout=sys.stdout, stderr=sys.stderr)

    stdout = os.dup(sys.stdout.fileno())
    stderr = os.dup(sys.stderr.fileno())
    os.dup2(pager.stdin.fileno(), sys.stdout.fileno())
    if util.isatty(sys.stderr):
        os.dup2(pager.stdin.fileno(), sys.stderr.fileno())

    @atexit.register
    def killpager():
        pager.stdin.close()
        os.dup2(stdout, sys.stdout.fileno())
        os.dup2(stderr, sys.stderr.fileno())
        pager.wait()

def uisetup(ui):
    if ui.plain() or '--debugger' in sys.argv or not util.isatty(sys.stdout):
        return

    def pagecmd(orig, ui, options, cmd, cmdfunc):
        p = ui.config("pager", "pager", os.environ.get("PAGER"))

        if p:
            attend = ui.configlist('pager', 'attend', attended)
            auto = options['pager'] == 'auto'
            always = util.parsebool(options['pager'])
            if (always or auto and
                (cmd in attend or
                 (cmd not in ui.configlist('pager', 'ignore') and not attend))):
                ui.setconfig('ui', 'formatted', ui.formatted())
                ui.setconfig('ui', 'interactive', False)
                if util.safehasattr(signal, "SIGPIPE"):
                    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
                _runpager(p)
        return orig(ui, options, cmd, cmdfunc)

    extensions.wrapfunction(dispatch, '_runcommand', pagecmd)

def extsetup(ui):
    commands.globalopts.append(
        ('', 'pager', 'auto',
         _("when to paginate (boolean, always, auto, or never)"),
         _('TYPE')))

attended = ['annotate', 'cat', 'diff', 'export', 'glog', 'log', 'qdiff']
