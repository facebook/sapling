# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# drop - allows the user to drop changeset from the middle of a stack

"""drop specified changeset from the stack

This command drops specified changeset from the stack.
For example, given changeset stack

o C
|
o B
|
o A
|
o master

execution of `hg drop -r B` command will result in the following stack

o C
|
o A
|
o master

If the changeset to drop has multiple children branching off of it,
all of them (including their descendants) will be rebased
onto the parent changeset. Dropping changeset which are a result of a merge
(have two parent changesets) is not supported.
Root changesets cannot be dropped.

"""

from edenscm.mercurial import cmdutil, error, extensions, phases, registrar, scmutil
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

testedwith = "ships-with-fb-hgext"


def _checkextension(name, ui):
    try:
        return extensions.find(name)
    except KeyError:
        ui.warn(_("extension %s not found\n") % name)
        return None


def _showrev(ui, repo, revid):
    """pretty print the changeset to drop"""
    showopts = {
        "template": "Dropping changeset "
        '{shortest(node, 6)}{if(bookmarks, " ({bookmarks})")}'
        ": {desc|firstline}\n"
    }
    displayer = cmdutil.show_changeset(ui, repo, showopts)
    displayer.show(repo[revid])


def extsetup(ui):
    global rebasemod
    rebasemod = _checkextension("rebase", ui)


@command("drop", [("r", "rev", [], _("revision to drop"))], _("hg drop [OPTION] [REV]"))
def drop(ui, repo, *revs, **opts):
    """drop changeset from stack
    """
    if not rebasemod:
        raise error.Abort(_("required extensions not detected"))

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)

    revs = scmutil.revrange(repo, list(revs) + opts.get("rev"))
    if not revs:
        raise error.Abort(_("no revision to drop was provided"))

    # currently drop supports dropping only one changeset at a time
    if len(revs) > 1:
        raise error.Abort(_("only one revision can be dropped at a time"))

    revid = revs.first()
    changectx = repo[revid]
    if changectx.phase() == phases.public:
        raise error.Abort(_("public changeset which landed cannot be dropped"))

    parents = repo.revs("parents(%s)", revid)
    if len(parents) > 1:
        raise error.Abort(_("merge changeset cannot be dropped"))
    elif len(parents) == 0:
        raise error.Abort(_("root changeset cannot be dropped"))

    _showrev(ui, repo, revid)

    descendants = repo.revs("(%d::) - %d", revid, revid)
    parent = parents.first()
    with repo.wlock():
        with repo.lock():
            with repo.transaction("drop"):
                if len(descendants) > 0:
                    try:
                        rebasemod.rebase(ui, repo, dest=str(parent), rev=descendants)
                    except error.InterventionRequired:
                        ui.warn(
                            _(
                                "conflict occurred during drop: "
                                + "please fix it by running "
                                + "'hg rebase --continue', "
                                + "and then re-run 'hg drop'\n"
                            )
                        )
                        raise
                    scmutil.cleanupnodes(repo, [changectx.node()], "drop")
