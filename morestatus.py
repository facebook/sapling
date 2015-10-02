# morestatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Make status give a bit more context

This extension will wrap the status command to make it show more context about
the state of the repo
"""

from mercurial import  commands
from mercurial.extensions import wrapcommand
from mercurial.i18n import _

def extsetup(ui):
    if ui.configbool('morestatus', 'show', False) and not ui.plain():
        wrapcommand(commands.table, 'status', statuscmd)

def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Wrap the status command to barf out the state of the repository. States
    being mid histediting, mid bisecting, grafting, merging, etc.
    Output is to stderr to avoid breaking scripts.
    """
    def repoisin(operation):
        msg = '\n# The repository is in an unfinished *%s* state.\n'
        return ui.warn(_(msg % operation))

    ret = orig(ui, repo, *pats, **opts)

    if repo.vfs.exists('histedit-state'):
        repoisin('histedit')
    elif repo.vfs.exists('bisect.state'):
        repoisin('bisect')
    elif repo.vfs.exists('graftstate'):
        repoisin('graft')
    elif repo.vfs.exists('unshelverebasestate'):
        repoisin('unshelve')
    elif repo.vfs.exists('rebasestate'):
        repoisin('rebase')
    elif repo.vfs.exists('merge'):
        repoisin('merge')

    # TODO(cdelahousse): check to see if current bookmark needs updating. See
    # scmprompt.

    return ret
