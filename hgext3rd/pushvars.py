# pushvars.py -- extension for setting environment variables on the server-side
#                during pushes.
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

def reposetup(ui, repo):
    """
    The main pushvars functionality moved into core hg. However, the behavior
    of the core version differs from this extension, which originally would
    set the environment variables on the server by default when the extension
    was enabled. To keep that behavior, this extension now just sets the option.
    This makes the transition painless.
    """
    repo.ui.setconfig('push', 'pushvars.server', True)
