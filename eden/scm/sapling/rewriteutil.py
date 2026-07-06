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


from bindings import agentdetect

from . import error, mutation, node
from .i18n import _
from .node import short


def precheck(repo, revs, action="rewrite", checkobsolete=True, checkmerge=True):
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

    if (
        checkobsolete
        and mutation.enabled(repo)
        and not repo.ui.plain()
        and repo.ui.configbool("commit", "reject-modifying-obsolete", True)
    ):
        obsrevs = repo.revs("%ld and obsolete()", revs)
        if obsrevs:
            msg = _("changing an old version of a commit will diverge your stack")
            details = []
            for rev in obsrevs:
                ctx = repo[rev]
                fates = mutation.fate(repo, ctx.node())
                for succs, op in fates:
                    succids = ", ".join(short(s) for s in succs)
                    details.append("- %s -> %s (%s)" % (short(ctx.node()), succids, op))
                if not fates:
                    details.append("- %s is obsolete" % short(ctx.node()))
            if details:
                msg += ":\n" + "\n".join(details)
            hint = _("run '@prog@ sl' for the latest commit graph view")
            if agentdetect.is_agent():
                raise error.Abort(msg, hint=hint)
            else:
                repo.ui.warn(_("warning: %s\n") % msg)
                choice = repo.ui.promptchoice(
                    _("proceed with %s (Yn)? $$ &Yes $$ &No") % action, default=0
                )
                if choice != 0:
                    raise error.Abort(_("aborted by user"))
