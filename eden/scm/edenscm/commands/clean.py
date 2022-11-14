# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright (C) 2006 - Marco Barisione <marco@barisione.org>
#
# This is a small extension for Mercurial (https://mercurial-scm.org/)
# that removes files not known to mercurial
#
# This program was inspired by the "cvspurge" script contained in CVS
# utilities (http://www.red-bean.com/cvsutils/).
#
# For help on the usage of "hg clean" use:
#  hg help clean
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, see <http://www.gnu.org/licenses/>.

"""command to delete untracked files from the working directory"""
from __future__ import absolute_import

from .. import cmdutil, error, scmutil
from ..i18n import _
from .cmdtable import command


@command(
    "clean|purge",
    [
        ("a", "abort-on-err", None, _("abort if an error occurs")),
        ("", "all", None, _("delete ignored files too (DEPRECATED)")),
        ("", "ignored", None, _("delete ignored files too")),
        ("", "dirs", None, _("delete empty directories")),
        ("", "files", None, _("delete files")),
        ("p", "print", None, _("print filenames instead of deleting them")),
        (
            "0",
            "print0",
            None,
            _("end filenames with NUL, for use with xargs" " (implies -p/--print)"),
        ),
    ]
    + cmdutil.walkopts,
    _("[OPTION]... [DIR]..."),
)
def clean(ui, repo, *dirs, **opts):
    """delete untracked files

    Delete all untracked files in your working copy. Untracked files are files
    that are unknown to @Product@. They are marked with "?" when you run
    :prog:`status`.

    By default, :prog:`clean` implies ``--files``, so only untracked
    files are deleted. If you add ``--ignored``, ignored files are also
    deleted. If you add ``--dirs``, empty directories are deleted and
    ``--files`` is no longer implied.

    If directories are given on the command line, only files in these
    directories are considered.

    Caution: :prog:`clean` is irreversible. To avoid accidents, first
    perform a dry run with :prog:`clean --print`.
    """
    act = not opts.get("print")
    eol = "\n"
    if opts.get("print0"):
        eol = "\0"
        act = False  # --print0 implies --print
    removefiles = opts.get("files")
    removedirs = opts.get("dirs")
    removeignored = opts.get("ignored")
    if removeignored is None:
        removeignored = opts.get("all")
    if not removefiles and not removedirs:
        removefiles = True
        removedirs = ui.configbool("clean", "dirs-by-default", default=None)
        if removedirs is None:
            removedirs = ui.configbool("purge", "dirs-by-default")

    match = scmutil.match(repo[None], dirs, opts)

    files, dirs, errors = repo.dirstate._fs.purge(
        match, removefiles, removedirs, removeignored, not act
    )
    if act:
        for f in files:
            ui.note(_("removing file %s\n") % f)

        for f in dirs:
            ui.note(_("removing directory %s\n") % f)
    else:
        for f in files:
            ui.write("%s%s" % (f, eol))
        for f in dirs:
            ui.write("%s%s" % (f, eol))

    for m in errors:
        if opts.get("abort_on_err"):
            raise error.Abort(m)
        ui.warn(_("warning: %s\n") % m)
