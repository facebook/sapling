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

If you notice "BROKEN PIPE" error messages, you can disable them by
setting::

  [pager]
  quiet = True

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

import sys, os, signal, shlex, errno
from mercurial import commands, dispatch, util, extensions
from mercurial.i18n import _

def _runpager(p):
    if not hasattr(os, 'fork'):
        sys.stderr = sys.stdout = util.popen(p, 'wb')
        return
    fdin, fdout = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(fdin)
        os.dup2(fdout, sys.stdout.fileno())
        os.dup2(fdout, sys.stderr.fileno())
        os.close(fdout)
        return
    os.dup2(fdin, sys.stdin.fileno())
    os.close(fdin)
    os.close(fdout)
    try:
        os.execvp('/bin/sh', ['/bin/sh', '-c', p])
    except OSError, e:
        if e.errno == errno.ENOENT:
            # no /bin/sh, try executing the pager directly
            args = shlex.split(p)
            os.execvp(args[0], args)
        else:
            raise

def uisetup(ui):
    if ui.plain():
        return

    def pagecmd(orig, ui, options, cmd, cmdfunc):
        p = ui.config("pager", "pager", os.environ.get("PAGER"))
        if p and sys.stdout.isatty() and '--debugger' not in sys.argv:
            attend = ui.configlist('pager', 'attend', attended)
            auto = options['pager'] == 'auto'
            always = util.parsebool(options['pager'])
            if (always or auto and
                (cmd in attend or
                 (cmd not in ui.configlist('pager', 'ignore') and not attend))):
                ui.setconfig('ui', 'formatted', ui.formatted())
                ui.setconfig('ui', 'interactive', False)
                _runpager(p)
                if ui.configbool('pager', 'quiet'):
                    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
        return orig(ui, options, cmd, cmdfunc)

    extensions.wrapfunction(dispatch, '_runcommand', pagecmd)

def extsetup(ui):
    commands.globalopts.append(
        ('', 'pager', 'auto',
         _("when to paginate (boolean, always, auto, or never)"),
         _('TYPE')))

attended = ['annotate', 'cat', 'diff', 'export', 'glog', 'log', 'qdiff']
