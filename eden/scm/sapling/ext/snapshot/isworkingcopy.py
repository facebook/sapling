# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import error
from sapling.i18n import _

from .createremote import (
    getdefaultmaxuntrackedsize,
    parsemaxuntracked,
    parsemaxuntrackedbytes,
)
from .latest import _isworkingcopy


def cmd(ui, repo, csid=None, *pats, **opts):
    if csid is None:
        raise error.CommandError("snapshot isworkingcopy", _("missing snapshot id"))

    try:
        snapshot = repo.edenapi.fetchsnapshot(
            {
                "cs_id": bytes.fromhex(csid),
            },
        )
    except Exception:
        raise error.Abort(_("snapshot doesn't exist"))

    maxuntrackedsize = parsemaxuntracked(opts)
    maxuntrackedsizebytes = parsemaxuntrackedbytes(opts)

    # Use bytes-based limit if specified, otherwise fall back to MiB-based limit, then config default
    effective_max_untracked_size = (
        maxuntrackedsizebytes or maxuntrackedsize or getdefaultmaxuntrackedsize(ui)
    )

    iswc, reason, wc = _isworkingcopy(
        ui, repo, snapshot, effective_max_untracked_size, pats, opts
    )

    # Use formatter for JSON output support and template support for automation
    if opts.get("template"):
        with ui.formatter("snapshot", opts) as fm:
            fm.startitem()
            fm.data(id=csid)
            fm.data(is_working_copy=iswc)
            if not iswc:
                fm.data(reason=reason)

            # Add skipped large untracked files to JSON output
            if wc and wc.skipped_large_untracked:
                fm.data(skipped_large_untracked=wc.skipped_large_untracked)

            # For non-JSON/template output, still show human-readable text
            if not ui.quiet and not ui.plain():
                if iswc:
                    fm.plain(_("snapshot {} is the working copy\n").format(csid))
                else:
                    fm.plain(
                        _("snapshot {} is not the working copy: {}\n").format(
                            csid, reason
                        )
                    )
    else:
        # Legacy output - abort on mismatch for non-template mode: to be deprecated in flavor of
        # better separation of output and error handling
        if iswc:
            if not ui.plain():
                ui.status(_("snapshot is the working copy\n"))
        else:
            raise error.Abort(_("snapshot is not the working copy: {}").format(reason))
