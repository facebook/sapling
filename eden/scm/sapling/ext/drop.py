# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# drop - allows the user to drop changesets from the middle of a stack

"""drop specified changesets from the stack

This command drops specified changesets from the stack.
For example, given changeset stack

o D
|
o C
|
o B
|
o A
|
o master

execution of `@prog@ drop -r B` command will result in the following stack

o D
|
o C
|
o A
|
o master

and `@prog@ drop -r B -r D` will result in

o C
|
o A
|
o master

If a changeset to drop has multiple children branching off of it,
all of them (including their descendants) will be rebased
onto the nearest ancestor that is not being dropped. Dropping changesets
which are a result of a merge (have two parent changesets) is not supported.
Root changesets cannot be dropped.

"""

from collections import defaultdict

from sapling import cmdutil, error, extensions, hg, registrar, revsetlang, scmutil
from sapling.i18n import _
from sapling.node import short


cmdtable = {}
command = registrar.command(cmdtable)

testedwith = "ships-with-fb-ext"


def _rebasemod():
    try:
        return extensions.find("rebase")
    except KeyError:
        raise error.Abort(_("the drop command requires the rebase extension"))


def _showrevs(ui, repo, nodes) -> None:
    """pretty print the changesets to drop"""
    showopts = {
        "template": "dropping changeset "
        '{shortest(node, 6)}{if(bookmarks, " ({bookmarks})")}'
        ": {desc|firstline}\n"
    }
    displayer = cmdutil.show_changeset(ui, repo, showopts)
    for node in nodes:
        displayer.show(repo[node])


def _latest(repo, node):
    """return the visible, non-obsolete successor of node, or node itself"""
    succs = list(repo.nodes("successors(%n) - obsolete()", node))
    if len(succs) == 1:
        return succs[0]
    return node


@command(
    "drop",
    [
        ("r", "rev", [], _("revisions to drop")),
        ("t", "tool", "", _("specify merge tool for rebasing descendants")),
    ],
    _("@prog@ drop -r REV..."),
)
def drop(ui, repo, *pats, **opts) -> None:
    """drop changesets from stack

    Remove the specified changesets from the stack, rebasing their
    descendants onto the nearest ancestor that is not dropped. Dropped
    changesets are hidden, bookmarks pointing to them are moved to that
    ancestor, and the working copy is moved there too if its parent was
    dropped.

    If a conflict occurs while rebasing descendants, resolve it and run
    :prog:`rebase --continue`, then re-run the :prog:`drop` command
    to hide the dropped changesets.
    """
    if pats:
        raise error.Abort(
            _("revisions to drop must be given with -r"),
            hint=_("use '@prog@ drop -r %s'") % " -r ".join(pats),
        )
    rebasemod = _rebasemod()

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)

    revs = scmutil.revrange(repo, opts.get("rev"))
    if not revs:
        raise error.Abort(_("no revision to drop was provided"))

    dropnodes = list(repo.nodes("sort(%ld)", revs))
    dropset = set(dropnodes)
    for node in dropnodes:
        ctx = repo[node]
        if ctx.ispublic():
            raise error.Abort(_("public changeset cannot be dropped: %s") % ctx)
        parents = ctx.parents()
        if len(parents) > 1:
            raise error.Abort(_("merge changeset cannot be dropped: %s") % ctx)
        if not parents:
            raise error.Abort(_("root changeset cannot be dropped: %s") % ctx)

    def keptancestor(node):
        """nearest first-parent ancestor of node that is not being dropped"""
        while node in dropset:
            node = repo.changelog.parents(node)[0]
        return node

    _showrevs(ui, repo, dropnodes)

    with repo.wlock(), repo.lock():
        descendants = list(repo.nodes("(%ln::) - %ln", dropnodes, dropnodes))
        # Skip obsolete descendants with nothing live on top of them, such as
        # commits already rebased by a drop that stopped on a conflict.
        rebasenodes = list(
            repo.nodes(
                "sort(%ln - (obsolete() - ::(%ln - obsolete())))",
                descendants,
                descendants,
            )
        )
        if rebasenodes:
            # Each child of a dropped changeset moves to the nearest kept
            # ancestor. Other descendants share their parent's destination,
            # so dropping a single changeset is a single-destination rebase;
            # rebase adjusts destinations that are themselves being rebased.
            destof = {}
            for node in rebasenodes:
                p1 = repo.changelog.parents(node)[0]
                destof[node] = destof.get(p1) or keptancestor(p1)
            bydest = defaultdict(list)
            for node, dest in destof.items():
                bydest[dest].append(node)
            dests = list(bydest)
            try:
                rebasemod.rebase(
                    ui,
                    repo,
                    rev=[revsetlang.formatspec("%ln", bydest[d]) for d in dests],
                    dest=[revsetlang.formatspec("%n", d) for d in dests],
                    tool=opts.get("tool"),
                )
            except error.InterventionRequired:
                ui.warn(
                    _(
                        "conflict occurred during drop: "
                        "please fix it by running "
                        "'@prog@ rebase --continue', "
                        "and then re-run '@prog@ drop %s'\n"
                    )
                    % " ".join("-r %s" % short(n) for n in dropnodes)
                )
                raise

        moves = {n: _latest(repo, keptancestor(n)) for n in dropnodes}
        wcp = repo["."].node()
        if wcp in dropset:
            hg.update(repo, moves[wcp], False)
            ui.status(
                _("working directory now at %s\n") % ui.label(short(moves[wcp]), "node")
            )
        scmutil.cleanupnodes(repo, dropnodes, "drop", moves=moves)
