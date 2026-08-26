# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# rewriteutil.py - utility functions for rewriting changesets
#
# Copyright 2017 Octobus <contact@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from contextlib import contextmanager

from bindings import agentdetect

from . import error, mutation, node, slacl
from .i18n import _
from .node import short

_SPLIT_IN_PROGRESS = "rewriteutil.split-in-progress"
_OBSOLETE_REWRITE_APPROVALS = "rewriteutil.obsolete-rewrite-approvals"


def precheck(repo, revs, action="rewrite", checkmerge=True):
    """check if revs can be rewritten
    action is used to control the error message.

    Make sure this function is called after taking the lock.
    """
    if node.nullrev in revs:
        msg = _("cannot %s null changeset") % action
        hint = _("no changeset checked out")
        raise error.Abort(msg, hint=hint)

    publicrevs = repo.revs("%ld and public()", revs)
    if checkmerge and len(repo.working_parent_nodes()) > 1:
        raise error.Abort(_("cannot %s while merging") % action)

    if publicrevs:
        msg = _("cannot %s public changesets") % action
        hint = _("see '@prog@ help phases' for details")
        raise error.Abort(msg, hint=hint)

    slacl.abort_if_restricted(repo, (repo[rev] for rev in revs))

    _record_obsolete_approvals(
        repo, _checkobsolete(repo, [repo[rev] for rev in revs], action)
    )


def _record_obsolete_approvals(repo, approved):
    if not approved:
        return
    if _OBSOLETE_REWRITE_APPROVALS not in repo.volatile_state:
        repo.volatile_state[_OBSOLETE_REWRITE_APPROVALS] = set()
        repo.ui.atexit(
            lambda: repo.volatile_state.pop(_OBSOLETE_REWRITE_APPROVALS, None)
        )
    approvals = repo.volatile_state[_OBSOLETE_REWRITE_APPROVALS]
    approvals.update(ctx.node() for ctx in approved)


def _checkobsolete(repo, contexts, action):
    if (
        not mutation.enabled(repo)
        or repo.ui.plain()
        or not repo.ui.configbool("commit", "reject-modifying-obsolete", True)
        # allowdivergence is the established opt-in for operations that
        # deliberately rewrite obsolete commits.
        or repo.ui.configbool("experimental", "evolution.allowdivergence")
    ):
        return []

    obsolete = []
    for ctx in contexts:
        if not ctx.obsolete():
            continue
        fates = mutation.fate(repo, ctx.node())
        obsolete.append((ctx, fates))
    if not obsolete:
        return []

    msg = _("changing an old version of a commit will diverge your stack")
    details = []
    for ctx, fates in obsolete:
        for successors, operation in fates:
            successor_ids = ", ".join(short(successor) for successor in successors)
            details.append(
                "- %s -> %s (%s)" % (short(ctx.node()), successor_ids, operation)
            )
        if not fates:
            details.append("- %s is obsolete" % short(ctx.node()))
    if details:
        msg += ":\n" + "\n".join(details)

    hint = _(
        "switch to the newer version listed above, or run '@prog@ graft' with "
        "the old commit hash to deliberately fork it; '@prog@ sl' shows the "
        "latest graph"
    )
    if agentdetect.is_agent():
        raise error.Abort(msg, hint=hint)

    repo.ui.warn(_("warning: %s\n") % msg)
    choice = repo.ui.promptchoice(
        _("proceed with %s (Yn)? $$ &Yes $$ &No") % action, default=0
    )
    if choice != 0:
        raise error.Abort(_("aborted by user"))
    return [ctx for ctx, _fates in obsolete]


@contextmanager
def splitting(repo):
    # Intermediate split commits lack mutation metadata. Mark the split in
    # progress so hooks can defer validation until the terminal split commit.
    if _SPLIT_IN_PROGRESS in repo.volatile_state:
        raise error.ProgrammingError("split is already in progress")
    repo.volatile_state[_SPLIT_IN_PROGRESS] = True
    try:
        yield
    finally:
        repo.volatile_state.pop(_SPLIT_IN_PROGRESS)


def issplitting(repo):
    return _SPLIT_IN_PROGRESS in repo.volatile_state


def _localcontexts(repo, nodes):
    return [
        repo[commit_node]
        for commit_node in repo.changelog.filternodes(nodes, local=True)
    ]


def commitcheck(repo, ctx):
    """Run extension checks for one commit being written."""
    mutinfo = ctx.mutinfo() or {}
    predecessors = _localcontexts(
        repo, mutation.nodesfrominfo(mutinfo.get("mutpred")) or []
    )
    split_successors = _localcontexts(
        repo, mutation.nodesfrominfo(mutinfo.get("mutsplit")) or []
    )
    # An approval covers the whole command: one approved predecessor may be
    # rewritten into several successors (e.g. a split), each reaching here.
    approvals = repo.volatile_state.get(_OBSOLETE_REWRITE_APPROVALS, set())
    unapproved_predecessors = [
        predecessor
        for predecessor in predecessors
        if predecessor.node() not in approvals
    ]
    # Record approvals granted here too, so rewrite paths without a command
    # precheck also prompt at most once per predecessor.
    _record_obsolete_approvals(
        repo, _checkobsolete(repo, unapproved_predecessors, "rewrite")
    )
    return predecessors, split_successors
