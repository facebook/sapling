# morestatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""make status give a bit more context

This extension will wrap the status command to make it show more context about
the state of the repo
"""

import os

from edenscm.mercurial import commands, merge as mergemod, pycompat, registrar, scmutil
from edenscm.mercurial.extensions import wrapcommand
from edenscm.mercurial.i18n import _


UPDATEARGS = "updateargs"

configtable = {}
configitem = registrar.configitem(configtable)

configitem("morestatus", "show", default=False)


def prefixlines(raw):
    """Surround lineswith a comment char and a new line"""
    lines = raw.splitlines()
    commentedlines = ["# %s" % line for line in lines]
    return "\n".join(commentedlines) + "\n"


def conflictsmsg(repo, ui):
    mergestate = mergemod.mergestate.read(repo)
    if not mergestate.active():
        return

    m = scmutil.match(repo[None])
    unresolvedlist = [f for f in mergestate if m(f) and mergestate[f] == "u"]
    if unresolvedlist:
        mergeliststr = "\n".join(
            [
                "    %s"
                % os.path.relpath(os.path.join(repo.root, path), pycompat.getcwd())
                for path in unresolvedlist
            ]
        )
        msg = (
            _(
                """Unresolved merge conflicts:

%s

To mark files as resolved:  hg resolve --mark FILE"""
            )
            % mergeliststr
        )
    else:
        msg = _("No unresolved merge conflicts.")

    ui.warn(prefixlines(msg))


def helpmessage(ui, continuecmd, abortcmd):
    msg = _("To continue:                %s\n" "To abort:                   %s") % (
        continuecmd,
        abortcmd,
    )
    ui.warn(prefixlines(msg))


def rebasemsg(repo, ui):
    helpmessage(ui, "hg rebase --continue", "hg rebase --abort")


def histeditmsg(repo, ui):
    helpmessage(ui, "hg histedit --continue", "hg histedit --abort")


def unshelvemsg(repo, ui):
    helpmessage(ui, "hg unshelve --continue", "hg unshelve --abort")


def updatecleanmsg(dest=None):
    warning = _("warning: this will discard uncommitted changes")
    return "hg update --clean %s    (%s)" % (dest or ".", warning)


def graftmsg(repo, ui):
    # tweakdefaults requires `update` to have a rev hence the `.`
    helpmessage(ui, "hg graft --continue", updatecleanmsg())


def updatemsg(repo, ui):
    previousargs = repo.localvfs.tryread(UPDATEARGS)
    if previousargs:
        continuecmd = "hg " + previousargs
    else:
        continuecmd = "hg update " + repo.localvfs.read("updatestate")[:12]
    abortcmd = updatecleanmsg(repo._activebookmark)
    helpmessage(ui, continuecmd, abortcmd)


def updatemergemsg(repo, ui):
    helpmessage(ui, "hg update --continue", updatecleanmsg())


def mergemsg(repo, ui):
    # tweakdefaults requires `update` to have a rev hence the `.`
    helpmessage(ui, "hg commit", updatecleanmsg())


def bisectmsg(repo, ui):
    msg = _(
        "To mark the changeset good:    hg bisect --good\n"
        "To mark the changeset bad:     hg bisect --bad\n"
        "To abort:                      hg bisect --reset\n"
    )
    ui.warn(prefixlines(msg))


def fileexistspredicate(filename):
    return lambda repo: repo.localvfs.exists(filename)


def mergepredicate(repo):
    return len(repo[None].parents()) > 1


STATES = (
    # (state, predicate to detect states, helpful message function)
    ("histedit", fileexistspredicate("histedit-state"), histeditmsg),
    ("bisect", fileexistspredicate("bisect.state"), bisectmsg),
    ("graft", fileexistspredicate("graftstate"), graftmsg),
    ("unshelve", fileexistspredicate("unshelverebasestate"), unshelvemsg),
    ("rebase", fileexistspredicate("rebasestate"), rebasemsg),
    # 'update --merge'. Unlike the 'update' state below, this can be
    # continued.
    ("update", fileexistspredicate("updatemergestate"), updatemergemsg),
    # The merge and update states are part of a list that will be iterated over.
    # They need to be last because some of the other unfinished states may also
    # be in a merge or update state (eg. rebase, histedit, graft, etc).
    # We want those to have priority.
    ("merge", mergepredicate, mergemsg),
    # Sometimes you end up in a merge state when update completes, because you
    # ran `hg update --merge`. We should inform you that you can still use the
    # full suite of resolve tools to deal with conflicts in this state.
    ("merge", fileexistspredicate("merge/state"), None),
    # If there were no conflicts, you may still be in an interrupted update
    # state. Ideally, we should expand this update state to include the merge
    # updates mentioned above, so there's a way to "continue" and finish the
    # update.
    ("update", fileexistspredicate("updatestate"), updatemsg),
)


def extsetup(ui):
    if ui.configbool("morestatus", "show") and not ui.plain():
        wrapcommand(commands.table, "status", statuscmd)
        # Write down `hg update` args to show the continue command in
        # interrupted update state.
        ui.setconfig("hooks", "pre-update.morestatus", saveupdateargs)
        ui.setconfig("hooks", "post-update.morestatus", cleanupdateargs)


def saveupdateargs(repo, args, **kwargs):
    # args is a string containing all flags and arguments
    with repo.wlock():
        repo.localvfs.write(UPDATEARGS, args)


def cleanupdateargs(repo, **kwargs):
    with repo.wlock():
        repo.localvfs.tryunlink(UPDATEARGS)


def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Wrap the status command to barf out the state of the repository. States
    being mid histediting, mid bisecting, grafting, merging, etc.
    Output is to stderr to avoid breaking scripts.
    """

    ret = orig(ui, repo, *pats, **opts)

    statetuple = getrepostate(repo)
    if statetuple:
        state, statedetectionpredicate, helpfulmsg = statetuple
        statemsg = _("The repository is in an unfinished *%s* state.") % state
        ui.warn("\n" + prefixlines(statemsg))
        conflictsmsg(repo, ui)
        if helpfulmsg:
            helpfulmsg(repo, ui)

    # TODO(cdelahousse): check to see if current bookmark needs updating. See
    # scmprompt.

    return ret


def getrepostate(repo):
    # experimental config: morestatus.skipstates
    skip = set(repo.ui.configlist("morestatus", "skipstates", []))
    for state, statedetectionpredicate, msgfn in STATES:
        if state in skip:
            continue
        if statedetectionpredicate(repo):
            return (state, statedetectionpredicate, msgfn)
