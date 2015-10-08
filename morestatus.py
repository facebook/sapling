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
from mercurial import merge as mergemod
from mercurial import scmutil


def prefixlines(raw):
    '''Surround lineswith a comment char and a new line'''
    lines = raw.splitlines()
    commentedlines = ['# %s' % line for line in lines]
    return '\n'.join(commentedlines) + '\n'


def conflictsmsg(repo, ui):
    mergestate = mergemod.mergestate(repo)
    if not mergestate.active():
        return

    m = scmutil.match(repo[None])
    unresolvedlist = [f for f in mergestate if m(f) and mergestate[f] == 'u']
    if unresolvedlist:
        mergeliststr = '\n'.join(['    %s' % path for path in unresolvedlist])
        msg = _('''Unresolved merge conflicts:

%s

To mark files as resolved:  hg resolve --mark FILE''') % mergeliststr
    else:
        msg = _('No unresolved merge conflicts.')

    ui.warn(prefixlines(msg))

def helpmessage(ui, continuecmd, abortcmd):
    msg = _('To continue:                %s\n'
            'To abort:                   %s') % (continuecmd, abortcmd)
    ui.warn(prefixlines(msg))

def rebasemsg(ui):
    helpmessage(ui, 'hg rebase --continue', 'hg rebase --abort')

def histeditmsg(ui):
    helpmessage(ui, 'hg histedit --continue', 'hg histedit --abort')

def unshelvemsg(ui):
    helpmessage(ui, 'hg unshelve --continue', 'hg unshelve --abort')

def graftmsg(ui):
     # tweakdefaults requires `update` to have a rev hence the `.`
    helpmessage(ui, 'hg graft --continue', 'hg update .')

def mergemsg(ui):
     # tweakdefaults requires `update` to have a rev hence the `.`
    helpmessage(ui, 'hg commit', 'hg update --clean .    (warning: this will '
            'erase all uncommitted changed)')

STATES = (
    # (state, file path indicating states, helpful message function)
    ('histedit', 'histedit-state', histeditmsg),
    ('bisect', 'bisect.state', None),
    ('graft', 'graftstate', graftmsg),
    ('unshelve', 'unshelverebasestate', unshelvemsg),
    ('rebase', 'rebasestate', rebasemsg),
    # The merge state is part of a list that will be iterated over. It needs to
    # be last because some of the other unfinished states may also be in a merge
    # state (eg.  histedit, graft, etc). We want those to have priority.
    ('merge', 'merge', mergemsg),
)

def extsetup(ui):
    if ui.configbool('morestatus', 'show', False) and not ui.plain():
        wrapcommand(commands.table, 'status', statuscmd)

def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Wrap the status command to barf out the state of the repository. States
    being mid histediting, mid bisecting, grafting, merging, etc.
    Output is to stderr to avoid breaking scripts.
    """

    ret = orig(ui, repo, *pats, **opts)

    statetuple = getrepostate(repo)
    if statetuple:
        state, statefile, helpfulmsg = statetuple
        statemsg = _('The repository is in an unfinished *%s* state.') % state
        ui.warn('\n' + prefixlines(statemsg))
        conflictsmsg(repo, ui)
        if helpfulmsg:
            helpfulmsg(ui)

    # TODO(cdelahousse): check to see if current bookmark needs updating. See
    # scmprompt.

    return ret

def getrepostate(repo):
    for state, statefilepath, msgfn in STATES:
        if repo.vfs.exists(statefilepath):
            return (state, statefilepath, msgfn)

