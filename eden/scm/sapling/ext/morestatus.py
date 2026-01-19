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

from bindings import workingcopy as wc
from sapling import (
    commands,
    hbisect,
    localrepo,
    merge as mergemod,
    node as nodeutil,
    scmutil,
    util,
)
from sapling.error import Abort
from sapling.extensions import wrapcommand
from sapling.i18n import _

UPDATEARGS = "updateargs"


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
                "    %s" % os.path.relpath(os.path.join(repo.root, path), os.getcwd())
                for path in unresolvedlist
            ]
        )
        msg = _(
            """Unresolved merge conflicts (%d):

%s

To mark files as resolved:  @prog@ resolve --mark FILE"""
        ) % (len(unresolvedlist), mergeliststr)
    else:
        msg = _("No unresolved merge conflicts.")

    ui.warn(prefixlines(msg))


def helpmessage(ui, continuecmd, abortcmd, quitcmd=None):
    items = [
        _("To continue:                %s") % continuecmd,
        _("To abort:                   %s") % abortcmd,
        _("To quit:                    %s") % quitcmd if quitcmd else None,
    ]
    msg = "\n".join(filter(None, items))
    ui.warn(prefixlines(msg))


def rebasemsg(repo, ui):
    helpmessage(
        ui,
        _("@prog@ rebase --continue"),
        _("@prog@ rebase --abort"),
        _("@prog@ rebase --quit"),
    )
    dstnode, srcnode = repo.dirstate.parents()
    if srcnode != nodeutil.nullid:
        src = repo[srcnode]
        dst = repo[dstnode]

        # fmt: off
        msg = _(
            "\n"
            "Rebasing %s (%s)\n"
            "      to %s (%s)"
        ) % (
            src, src.shortdescription(),
            dst, dst.shortdescription(),
        )
        # fmt: on
        ui.write_err(prefixlines(msg))


def updatecleanmsg(dest=None):
    warning = _("warning: this will discard uncommitted changes")
    return _("@prog@ goto %s --clean    (%s)") % (dest or ".", warning)


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


def mergestate2msg(repo, ui):
    helpmessage(ui, _("@prog@ continue, then @prog@ commit"), updatecleanmsg())


def bisectmsg(repo, ui):
    msg = _(
        "To mark the commit good:     @prog@ bisect --good\n"
        "To mark the commit bad:      @prog@ bisect --bad\n"
        "To abort:                    @prog@ bisect --reset\n"
    )

    state = hbisect.load_state(repo)
    bisectstatus = _(
        """Current bisect state: {} good commit(s), {} bad commit(s), {} skip commit(s)"""
    ).format(len(state["good"]), len(state["bad"]), len(state["skip"]))
    ui.write_err(prefixlines(bisectstatus))

    if len(state["good"]) > 0 and len(state["bad"]) > 0:
        try:
            nodes, _untested, commitsremaining, badtogood, rightnode, leftnode = (
                hbisect.bisect(repo, state)
            )

            searchesremaining = (
                int(math.ceil(math.log(commitsremaining, 2)))
                if commitsremaining > 0
                else 0
            )
            bisectstatustmpl = _(
                """
Current Tracker: {:<15}{:<15}{}
                 {}...{}...{}
Commits remaining:           {}
Estimated bisects remaining: {}
"""
            )
            labels = ("good commit", "bad commit", "current")
            bisectstatus = bisectstatustmpl.format(
                labels[badtogood],
                labels[2],
                labels[1 - badtogood],
                nodeutil.short(leftnode),
                nodeutil.short(nodes[0]),
                nodeutil.short(rightnode),
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
    return len(repo.working_parent_nodes()) > 1


STATES = (
    # (state, rust state object with 'is_active()' and 'hint()')
    # OR
    # (state, predicate to detect states, helpful message function)
    ("histedit", wc.commandstate.get_state("histedit", "histedit-state")),
    ("bisect", fileexistspredicate("bisect.state"), bisectmsg),
    ("graft", wc.commandstate.get_state("graft", "graftstate")),
    ("unshelve", wc.commandstate.get_state("unshelve", "shelvedstate")),
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
    ("merge", fileexistspredicate("merge/state2"), mergestate2msg),
    # If there were no conflicts, you may still be in an interrupted update
    # state. Ideally, we should expand this update state to include the merge
    # updates mentioned above, so there's a way to "continue" and finish the
    # update.
    ("update", fileexistspredicate("updatestate"), updatemsg),
)


def extsetup(ui):
    wrapcommand(commands.table, "status", statuscmd)
    localrepo.localrepository._wlockfreeprefix.add(UPDATEARGS)


def reposetup(ui, repo):
    # Write down `hg update` args to show the continue command in
    # interrupted update state.
    ui.setconfig(
        "hooks", "pre-update.morestatus", "python:sapling.ext.morestatus.saveupdateargs"
    )
    ui.setconfig(
        "hooks",
        "post-update.morestatus",
        "python:sapling.ext.morestatus.cleanupdateargs",
    )


def saveupdateargs(repo, args, **kwargs) -> None:
    # args is a string containing all flags and arguments
    if isinstance(args, list):
        args = " ".join(map(util.shellquote, args))
    repo = getattr(repo, "_rsrepo", repo)
    with util.atomictempfile(os.path.join(repo.dot_path, UPDATEARGS), "wb") as fp:
        fp.write(args.encode("utf-8"))


def cleanupdateargs(repo, **kwargs) -> None:
    repo = getattr(repo, "_rsrepo", repo)
    util.tryunlink(os.path.join(repo.dot_path, UPDATEARGS))


def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Wrap the status command to barf out the state of the repository. States
    being mid histediting, mid bisecting, grafting, merging, etc.
    Output is to stderr to avoid breaking scripts.
    """

    ret = orig(ui, repo, *pats, **opts)
    if not ui.configbool("morestatus", "show") or ui.plain():
        return ret

    statetuple = getrepostate(repo)
    if statetuple:
        state, helpfulmsg = statetuple
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

    for compositestate in STATES:
        if len(compositestate) == 2:
            # rust case
            statename, state = compositestate
            if statename in skip:
                continue
            if state.is_active(repo.path):
                msgfn = lambda _repo, ui: ui.warn(prefixlines(state.hint()))
                return (statename, msgfn)
        elif len(compositestate) == 3:
            # python case
            statename, statedetectionpredicate, msgfn = compositestate
            if statename in skip:
                continue
            if statedetectionpredicate(repo):
                return (statename, msgfn)
        else:
            raise Abort(
                _(
                    "invalid command state configuration: expected tuple of length 2 or 3, got length %s"
                )
                % len(compositestate)
            )
