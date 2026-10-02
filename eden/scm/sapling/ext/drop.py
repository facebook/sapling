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
from sapling.node import bin, hex, short


cmdtable = {}
command = registrar.command(cmdtable)

testedwith = "ships-with-fb-ext"

# Records the changesets being dropped while a drop is interrupted, so that
# `drop --continue` can finish the drop after the rebase is resolved.
_STATEFILE = "dropstate"


def uisetup(ui) -> None:
    entry = (_STATEFILE, "@prog@ drop --continue")
    if entry not in cmdutil.afterresolvedstates:
        # An interrupted drop also leaves a rebase in progress, so this must be
        # checked before the rebase entry.
        cmdutil.afterresolvedstates.insert(0, entry)


def _rebasemod():
    try:
        return extensions.find("rebase")
    except KeyError:
        raise error.Abort(_("the drop command requires the rebase extension"))


def _writestate(repo, nodes) -> None:
    with repo.localvfs(_STATEFILE, "wb", atomictemp=True) as f:
        f.write("".join("%s\n" % hex(n) for n in nodes).encode())


def _readstate(repo):
    if not repo.localvfs.exists(_STATEFILE):
        raise error.Abort(_("no drop in progress"))
    return [bin(line) for line in repo.localvfs.read(_STATEFILE).decode().split()]


def _clearstate(repo) -> None:
    repo.localvfs.tryunlink(_STATEFILE)


def _conflict():
    return error.InterventionRequired(
        _("unresolved conflicts (see @prog@ resolve, then @prog@ drop --continue)")
    )


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
        ("", "continue", False, _("continue an interrupted drop")),
        ("", "abort", False, _("abort an interrupted drop")),
    ],
    _("@prog@ drop [OPTION]... -r REV..."),
)
def drop(ui, repo, *pats, **opts) -> None:
    """remove changesets from the middle of a stack

    Remove the specified changesets from the stack, rebasing their
    descendants onto the nearest ancestor that is not dropped. Dropped
    changesets are hidden, bookmarks pointing to them are moved to that
    ancestor, and the working copy is moved there too if its parent was
    dropped.

    If a conflict occurs while rebasing descendants, resolve it and run
    :prog:`drop --continue`, or run :prog:`drop --abort` to stop the drop.
    Aborting undoes the rebase of the descendants and leaves the
    changesets to drop in place.
    """
    if pats:
        raise error.Abort(
            _("revisions to drop must be given with -r"),
            hint=_("use '@prog@ drop -r %s'") % " -r ".join(pats),
        )
    rebasemod = _rebasemod()
    tool = opts.get("tool")

    if opts.get("continue") or opts.get("abort"):
        if opts.get("continue") and opts.get("abort"):
            raise error.Abort(_("cannot use both --continue and --abort"))
        if opts.get("rev"):
            raise error.Abort(_("cannot specify revisions with --continue or --abort"))
        with repo.wlock(), repo.lock():
            dropnodes = _readstate(repo)
            if opts.get("abort"):
                if repo.localvfs.exists("rebasestate"):
                    rebasemod.rebase(ui, repo, abort=True)
                _clearstate(repo)
                ui.status(_("drop aborted\n"))
                return
            if repo.localvfs.exists("rebasestate"):
                try:
                    rebasemod.rebase(ui, repo, tool=tool, **{"continue": True})
                except error.InterventionRequired:
                    raise _conflict()
            _finishdrop(ui, repo, rebasemod, dropnodes, tool)
        return

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)

    revs = scmutil.revrange(repo, opts.get("rev"))
    if not revs:
        raise error.Abort(_("no revision to drop was provided"))

    dropnodes = list(repo.nodes("sort(%ld)", revs))
    for node in dropnodes:
        ctx = repo[node]
        if ctx.ispublic():
            raise error.Abort(_("public changeset cannot be dropped: %s") % ctx)
        parents = ctx.parents()
        if len(parents) > 1:
            raise error.Abort(_("merge changeset cannot be dropped: %s") % ctx)
        if not parents:
            raise error.Abort(_("root changeset cannot be dropped: %s") % ctx)

    _showrevs(ui, repo, dropnodes)

    with repo.wlock(), repo.lock():
        _finishdrop(ui, repo, rebasemod, dropnodes, tool)


def _finishdrop(ui, repo, rebasemod, dropnodes, tool) -> None:
    """rebase the descendants of dropnodes that still need it, then hide
    dropnodes"""
    dropset = set(dropnodes)

    def keptancestor(node):
        """nearest first-parent ancestor of node that is not being dropped"""
        while node in dropset:
            node = repo.changelog.parents(node)[0]
        return node

    descendants = list(repo.nodes("(%ln::) - %ln", dropnodes, dropnodes))
    # Skip obsolete descendants with nothing live on top of them, such as
    # commits already rebased before the drop was interrupted.
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

        # A new rebase refuses to start while an unfinished drop is recorded.
        _clearstate(repo)
        try:
            rebasemod.rebase(
                ui,
                repo,
                rev=[revsetlang.formatspec("%ln", bydest[d]) for d in dests],
                dest=[revsetlang.formatspec("%n", d) for d in dests],
                tool=tool,
            )
        except error.InterventionRequired:
            _writestate(repo, dropnodes)
            raise _conflict()
        except BaseException:
            if repo.localvfs.exists("rebasestate"):
                _writestate(repo, dropnodes)
            raise

    moves = {n: _latest(repo, keptancestor(n)) for n in dropnodes}
    wcp = repo["."].node()
    if wcp in dropset:
        hg.update(repo, moves[wcp], False)
        ui.status(
            _("working directory now at %s\n") % ui.label(short(moves[wcp]), "node")
        )
    scmutil.cleanupnodes(repo, dropnodes, "drop", moves=moves)
    _clearstate(repo)
