# scmutil.py - Mercurial core utility functions
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util, error
import os

def checkportable(ui, f):
    '''Check if filename f is portable and warn or abort depending on config'''
    util.checkfilename(f)
    val = ui.config('ui', 'portablefilenames', 'warn')
    lval = val.lower()
    abort = os.name == 'nt' or lval == 'abort'
    bval = util.parsebool(val)
    if abort or lval == 'warn' or bval:
        msg = util.checkwinfilename(f)
        if msg:
            if abort:
                raise util.Abort("%s: %r" % (msg, f))
            ui.warn(_("warning: %s: %r\n") % (msg, f))
    elif bval is None and lval != 'ignore':
        raise error.ConfigError(
            _("ui.portablefilenames value is invalid ('%s')") % val)
