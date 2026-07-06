# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import bindings

from . import error
from .i18n import _


_MIXED_COMMIT_MODE_CONFIG = "slacl.mixed-commit-mode"


def _permission_denied_rctx(repo):
    rctx = getattr(getattr(repo.ui, "_uiconfig", None), "_rctx", None)
    if rctx is None:
        repo.ui.debug(
            "slacl rewrite precheck has no permission-denied record context\n"
        )
    return rctx


def _permission_denied_count(repo):
    rctx = _permission_denied_rctx(repo)
    if rctx is None:
        return 0
    return rctx.permission_denied_count()


def _permission_denied_details(repo):
    rctx = _permission_denied_rctx(repo)
    if rctx is None:
        return []
    _warning, acl_details, _exit_nonzero = bindings.context.check_permission_denied(
        rctx
    )
    return acl_details


def abort_if_restricted(repo, ctxs):
    """Abort when rewriting commits whose p1 diff contains restricted paths."""
    mode = repo.ui.config("slacl", "mixed-commit-mode", "abort")
    if mode == "ignore":
        return
    if mode not in ("abort", "warn"):
        raise error.Abort(
            _("invalid value for %s: %s") % (_MIXED_COMMIT_MODE_CONFIG, mode),
            hint=_("valid values are 'abort', 'warn', and 'ignore'"),
        )

    ctxs = list(ctxs)
    before = _permission_denied_count(repo)
    bindings.manifest.prefetch_diff(
        [(ctx.manifest(), ctx.p1().manifest()) for ctx in ctxs]
    )
    if _permission_denied_count(repo) == before:
        return

    details = _permission_denied_details(repo)
    if mode == "warn":
        message = (
            _("warning: rewriting commits with restricted paths (%s=warn)")
            % _MIXED_COMMIT_MODE_CONFIG
        )
        if details:
            message = message + "\n" + "".join(details).rstrip()
        repo.ui.warn(message + "\n")
        return

    message = _("cannot rewrite commits with restricted paths")
    if details:
        message = message + "\n" + "".join(details).rstrip()
    ex = error.Abort(
        message,
        hint=_("use '--config %s=warn' to bypass") % _MIXED_COMMIT_MODE_CONFIG,
    )
    ex.permission_denied_handled = True
    raise ex
