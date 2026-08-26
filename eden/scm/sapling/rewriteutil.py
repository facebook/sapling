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


def _hasvisibledescendant(repo, ctx):
    # wdir() is a virtual descendant of the checked-out commit, not an
    # existing committed stack.
    descendants = repo.nodes(
        "limit((((%n::) - %n) - hidden()) - wdir(), 1)",
        ctx.node(),
        ctx.node(),
    )
    return next(iter(descendants), None) is not None


def _obsolete_mode(repo, config):
    default = "abort" if agentdetect.is_agent() else "warn"
    mode = repo.ui.config(config[0], config[1], default)
    return mode if mode in ("warn", "abort") else "ignore"


def _checkobsolete(
    repo,
    contexts,
    action,
    message=None,
    hint=None,
    allow_public_successors=False,
    allow_visible_descendants=False,
    config=("commit", "reject-modifying-obsolete"),
    mode_config=None,
):
    if (
        not mutation.enabled(repo)
        or repo.ui.plain()
        # allowdivergence is the established opt-in for operations that
        # deliberately rewrite obsolete commits.
        or repo.ui.configbool("experimental", "evolution.allowdivergence")
    ):
        return []

    ispublic = mutation.getispublicfunc(repo) if allow_public_successors else None
    obsolete = []
    for ctx in contexts:
        if not ctx.obsolete():
            continue
        if allow_visible_descendants and _hasvisibledescendant(repo, ctx):
            continue
        fates = mutation.fate(repo, ctx.node())
        # After every successor lands, working on top of the old draft is a
        # routine post-land state rather than new divergence. A fate with no
        # successors (e.g. a prune) is not "landed", so it does not qualify.
        if (
            allow_public_successors
            and fates
            and all(
                successors and all(ispublic(successor) for successor in successors)
                for successors, _operation in fates
            )
        ):
            continue
        obsolete.append((ctx, fates))
    if not obsolete:
        return []

    if mode_config is None:
        if not repo.ui.configbool(config[0], config[1], True):
            return []
        mode = "abort" if agentdetect.is_agent() else "prompt"
    else:
        mode = _obsolete_mode(repo, mode_config)
        if mode == "ignore":
            return []

    if message is None:
        message = _("changing an old version of a commit will diverge your stack")
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
        message += ":\n" + "\n".join(details)

    if hint is None:
        hint = _(
            "switch to the newer version listed above, or run '@prog@ graft' "
            "with the old commit hash to deliberately fork it; '@prog@ sl' "
            "shows the latest graph"
        )
    if mode == "abort":
        raise error.Abort(message, hint=hint)

    repo.ui.warn(_("warning: %s\n") % message)
    if mode == "warn":
        return [ctx for ctx, _fates in obsolete]

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
    predecessor_parents = {
        parent.node()
        for predecessor in predecessors
        for parent in predecessor.parents()
    }
    _checkobsolete(
        repo,
        [
            parent
            for parent in ctx.parents()
            if parent.node() != node.nullid and parent.node() not in predecessor_parents
        ],
        "commit",
        _("creating a child of an old version of a commit will diverge your stack"),
        _(
            "switch to the newer version listed above first -- '@prog@ goto' "
            "carries uncommitted changes along, or use '@prog@ shelve' and "
            "'@prog@ unshelve' to move them"
        ),
        allow_public_successors=not predecessors,
        allow_visible_descendants=not predecessors,
    )
    return predecessors, split_successors


def gotocheck(repo, targets):
    """Guard checkouts of hidden obsolete commits.

    Depending on `checkout.obsolete-mode` and whether the caller is an agent,
    this warns, prompts, or aborts; visible obsolete commits are not guarded.
    """
    hidden_nodes = set(repo.nodes("%ln & hidden()", [ctx.node() for ctx in targets]))
    hidden_targets = [ctx for ctx in targets if ctx.node() in hidden_nodes]
    if not hidden_targets:
        return
    target_args = " ".join(short(ctx.node()) for ctx in hidden_targets)
    _checkobsolete(
        repo,
        hidden_targets,
        "goto",
        _("checking out an old version of a commit risks diverging your stack"),
        _(
            "check out the newer version listed above instead, or run "
            "'@prog@ unhide %s' to explicitly allow checking out this old commit"
        )
        % target_args,
        allow_public_successors=True,
        allow_visible_descendants=True,
        mode_config=("checkout", "obsolete-mode"),
    )
