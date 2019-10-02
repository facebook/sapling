# Copyright (C) 2006 - Marco Barisione <marco@barisione.org>
#
# This is a small extension for Mercurial (https://mercurial-scm.org/)
# that removes files not known to mercurial
#
# This program was inspired by the "cvspurge" script contained in CVS
# utilities (http://www.red-bean.com/cvsutils/).
#
# For help on the usage of "hg purge" use:
#  hg help purge
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

import os

from edenscm.mercurial import cmdutil, error, registrar, scmutil, util
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"


def findthingstopurge(repo, match, findfiles, finddirs, includeignored):
    """Find files and/or directories that should be purged.

    Returns a pair (files, dirs), where files is an iterable of files to
    remove, and dirs is an iterable of directories to remove.
    """
    if finddirs:
        directories = [f for f in match.files() if repo.wvfs.isdir(f)]
        match.traversedir = directories.append

    status = repo.status(match=match, ignored=includeignored, unknown=True)

    if findfiles:
        files = sorted(status.unknown + status.ignored)
    else:
        files = []

    if finddirs:
        # Use a generator expression to lazily test for directory contents,
        # otherwise nested directories that are being removed would be counted
        # when in reality they'd be removed already by the time the parent
        # directory is to be removed.
        dirs = (
            f
            for f in sorted(directories, reverse=True)
            if (match(f) and not os.listdir(repo.wjoin(f)))
        )
    else:
        dirs = []

    return files, dirs


@command(
    "purge|clean",
    [
        ("a", "abort-on-err", None, _("abort if an error occurs")),
        ("", "all", None, _("purge ignored files too")),
        ("", "dirs", None, _("purge empty directories")),
        ("", "files", None, _("purge files")),
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
def purge(ui, repo, *dirs, **opts):
    """delete untracked files

    Delete all untracked files in your checkout. Untracked files are files
    that are unknown to Mercurial. They are marked with "?" when you run
    :hg:`status`.

    By default, :hg:`purge` does not affect::

    - Modified and unmodified tracked files
    - Ignored files (unless --all is specified)
    - New files added to the repository with :hg:`add`, but not yet committed
    - Empty directories that contain no files (unless --dirs is specified)

    If directories are given on the command line, only files in these
    directories are considered.

    Caution: Be careful with purge, as you might irreversibly delete some files
    you forgot to add to the repository. There is no way to undo an
    :hg:`purge` operation. Run :hg:`status` first to verify the list of
    files that will be deleted, or use the --print option with :hg:`purge`
    to preview the results.
    """
    act = not opts.get("print")
    eol = "\n"
    if opts.get("print0"):
        eol = "\0"
        act = False  # --print0 implies --print
    removefiles = opts.get("files")
    removedirs = opts.get("dirs")
    removeignored = opts.get("all")
    if not removefiles and not removedirs:
        removefiles = True
        removedirs = True

    def remove(remove_func, name):
        if act:
            try:
                remove_func(repo.wjoin(name))
            except OSError:
                m = _("%s cannot be removed") % name
                if opts.get("abort_on_err"):
                    raise error.Abort(m)
                ui.warn(_("warning: %s\n") % m)
        else:
            ui.write("%s%s" % (name, eol))

    match = scmutil.match(repo[None], dirs, opts)
    files, dirs = findthingstopurge(repo, match, removefiles, removedirs, removeignored)

    for f in files:
        if act:
            ui.note(_("removing file %s\n") % f)
        remove(util.unlink, f)

    for f in dirs:
        if act:
            ui.note(_("removing directory %s\n") % f)
        remove(os.rmdir, f)
