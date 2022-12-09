# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""make status give a bit more context

This extension will wrap the status command to make it show more context about
the state of the repo
"""

import math
import os

from edenscm import (
    commands,
    hbisect,
    merge as mergemod,
    node as nodeutil,
    pycompat,
    registrar,
    scmutil,
)
from edenscm.error import Abort
from edenscm.extensions import wrapcommand
from edenscm.i18n import _


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

To mark files as resolved:  @prog@ resolve --mark FILE"""
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
    helpmessage(ui, _("@prog@ rebase --continue"), _("@prog@ rebase --abort"))
    dstnode, srcnode = repo.dirstate.parents()
    if srcnode != nodeutil.nullid:
        src = repo[srcnode]
        dst = repo[dstnode]
        msg = _("\nRebasing from %s (%s)\n           to %s (%s)") % (
            src,
            src.shortdescription(),
            dst,
            dst.shortdescription(),
        )
        ui.write_err(prefixlines(msg))


def histeditmsg(repo, ui):
    helpmessage(ui, _("@prog@ histedit --continue"), _("@prog@ histedit --abort"))


def unshelvemsg(repo, ui):
    helpmessage(ui, _("@prog@ unshelve --continue"), _("@prog@ unshelve --abort"))


def updatecleanmsg(dest=None):
    warning = _("warning: this will discard uncommitted changes")
    return _("@prog@ goto --clean %s    (%s)") % (dest or ".", warning)


def graftmsg(repo, ui):
    # tweakdefaults requires `update` to have a rev hence the `.`
    helpmessage(ui, _("@prog@ graft --continue"), updatecleanmsg())


def updatemsg(repo, ui):
    previousargs = repo.localvfs.tryreadutf8(UPDATEARGS)
    if previousargs:
        continuecmd = _("@prog@ ") + previousargs
    else:
        continuecmd = _("@prog@ goto ") + repo.localvfs.readutf8("updatestate")[:12]
    abortcmd = updatecleanmsg(repo._activebookmark)
    helpmessage(ui, continuecmd, abortcmd)


def updatemergemsg(repo, ui):
    helpmessage(ui, _("@prog@ goto --continue"), updatecleanmsg())


def mergemsg(repo, ui):
    # tweakdefaults requires `update` to have a rev hence the `.`
    helpmessage(ui, _("@prog@ commit"), updatecleanmsg())


def bisectmsg(repo, ui):
    msg = _(
        "To mark the changeset good:    @prog@ bisect --good\n"
        "To mark the changeset bad:     @prog@ bisect --bad\n"
        "To abort:                      @prog@ bisect --reset\n"
    )

    state = hbisect.load_state(repo)
    bisectstatus = _(
        """Current bisect state: {} good commit(s), {} bad commit(s), {} skip commit(s)"""
    ).format(len(state["good"]), len(state["bad"]), len(state["skip"]))
    ui.write_err(prefixlines(bisectstatus))

    if len(state["good"]) > 0 and len(state["bad"]) > 0:
        try:
            nodes, commitsremaining, searching, badnode, goodnode = hbisect.bisect(
                repo, state
            )
            searchesremaining = (
                int(math.ceil(math.log(commitsremaining, 2)))
                if commitsremaining > 0
                else 0
            )
            bisectstatus = _(
                """
Current Tracker: bad commit     current        good commit
                 {}...{}...{}
Commits remaining:           {}
Estimated bisects remaining: {}
"""
            ).format(
                nodeutil.short(badnode),
                nodeutil.short(nodes[0]),
                nodeutil.short(goodnode),
                commitsremaining,
                searchesremaining,
            )

            ui.write_err(prefixlines(bisectstatus))
        except Abort:
            # ignore the output if bisect() fails
            pass
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
        repo.localvfs.writeutf8(UPDATEARGS, args)


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
