# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

'''share a common history between several working directories'''

from mercurial.i18n import _
from mercurial import hg, commands

def share(ui, source, dest=None, noupdate=False):
    """create a new shared repository (experimental)

    Initialize a new repository and working directory that shares its
    history with another repository.

    NOTE: actions that change history such as rollback or moving the
    source may confuse sharers.
    """

    return hg.share(ui, source, dest, not noupdate)

cmdtable = {
    "share":
    (share,
     [('U', 'noupdate', None, _('do not create a working copy'))],
     _('[-U] SOURCE [DEST]')),
}

commands.norepo += " share"
