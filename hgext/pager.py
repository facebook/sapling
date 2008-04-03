# pager.py - display output using a pager
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# To load the extension, add it to your .hgrc file:
#
#   [extension]
#   hgext.pager =
#
# To set the pager that should be used, set the application variable:
#
#   [pager]
#   pager = LESS='FSRX' less
#
# If no pager is set, the pager extensions uses the environment
# variable $PAGER. If neither pager.pager, nor $PAGER is set, no pager
# is used.
#
# If you notice "BROKEN PIPE" error messages, you can disable them
# by setting:
#
#   [pager]
#   quiet = True

import sys, os, signal

def uisetup(ui):
    p = ui.config("pager", "pager", os.environ.get("PAGER"))
    if p and sys.stdout.isatty() and '--debugger' not in sys.argv:
        if ui.configbool('pager', 'quiet'):
            signal.signal(signal.SIGPIPE, signal.SIG_DFL)
        sys.stderr = sys.stdout = os.popen(p, "wb")
