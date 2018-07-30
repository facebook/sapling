# record.py
#
# Copyright 2007 Bryan O'Sullivan <bos@serpentine.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""commands to interactively select changes for commit/qrefresh (DEPRECATED)

The feature provided by this extension has been moved into core Mercurial as
:hg:`commit --interactive`."""

from __future__ import absolute_import

from mercurial import cmdutil, commands, error, extensions, registrar
from mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"


@command(
    "record",
    # same options as commit + white space diff options
    [c for c in commands.table["^commit|ci"][1][:] if c[1] != "interactive"]
    + cmdutil.diffwsopts,
    _("hg record [OPTION]... [FILE]..."),
)
def record(ui, repo, *pats, **opts):
    """interactively select changes to commit

    If a list of files is omitted, all changes reported by :hg:`status`
    will be candidates for recording.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    If using the text interface (see :hg:`help config`),
    you will be prompted for whether to record changes to each
    modified file, and for files with multiple changes, for each
    change to use. For each query, the following responses are
    possible::

      y - record this change
      n - skip this change
      e - edit this change manually

      s - skip remaining changes to this file
      f - record remaining changes to this file

      d - done, skip remaining changes and files
      a - record all changes to all remaining files
      q - quit, recording no changes

      ? - display help

    This command is not available when committing a merge."""

    if not ui.interactive():
        raise error.Abort(_("running non-interactively, use %s instead") % "commit")

    opts[r"interactive"] = True
    overrides = {("experimental", "crecord"): False}
    with ui.configoverride(overrides, "record"):
        return commands.commit(ui, repo, *pats, **opts)
