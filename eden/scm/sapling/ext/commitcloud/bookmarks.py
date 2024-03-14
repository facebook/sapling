# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from sapling import commands, encoding, error, extensions, hg, pycompat, util
from sapling.i18n import _
from sapling.util import sortdict

from .util import scratchbranchmatcher


def extsetup(ui) -> None:
    bookcmd = extensions.wrapcommand(commands.table, "bookmark", _bookmarks)
    bookcmd[1].append(
        (
            "",
            "list-remote",
            None,
            "list remote bookmarks. "
            "Positional arguments are interpreted as wildcard patterns. "
            "Only allowed wildcard is '*' in the end of the pattern. "
            "If no positional arguments are specified then it will list "
            'the most "important" remote bookmarks. '
            "Otherwise it will list remote bookmarks "
            "that match at least one pattern "
            "",
        )
    )


def _bookmarks(orig, ui, repo, *names, **opts):
    pattern = opts.get("list_remote")
    delete = opts.get("delete")
    remotepath = opts.get("remote_path")
    path = ui.paths.getpath(remotepath or None, default=("default",))

    if pattern:
        destpath = path.pushloc or path.loc
        other = hg.peer(repo, opts, destpath)
        if not names:
            raise error.Abort(
                "--list-remote requires a bookmark pattern",
                hint=_('use "@prog@ book" to get a list of your local bookmarks'),
            )
        else:
            # prefix bookmark listing is not yet supported by Edenapi.
            usehttp = repo.ui.configbool("infinitepush", "httpbookmarks") and not any(
                n.endswith("*") for n in names
            )

            if usehttp:
                fetchedbookmarks = _http_bookmark_fetch(repo, names)
            else:
                fetchedbookmarks = other.listkeyspatterns("bookmarks", patterns=names)
        _showbookmarks(ui, fetchedbookmarks, **opts)
        return
    elif delete and "remotenames" in extensions._extensions:
        with repo.wlock(), repo.lock(), repo.transaction("bookmarks"):
            existing_local_bms = set(repo._bookmarks.keys())
            scratch_bms = []
            other_bms = []
            scratchmatcher = scratchbranchmatcher(ui)
            for name in names:
                if scratchmatcher.match(name) and name not in existing_local_bms:
                    scratch_bms.append(name)
                else:
                    other_bms.append(name)

            if len(scratch_bms) > 0:
                if remotepath == "":
                    remotepath = "default"
                _deleteremotebookmarks(ui, repo, remotepath, scratch_bms)

            if len(other_bms) > 0 or len(scratch_bms) == 0:
                return orig(ui, repo, *other_bms, **opts)
    else:
        return orig(ui, repo, *names, **opts)


def _showbookmarks(ui, remotebookmarks, **opts) -> None:
    # Copy-paste from commands.py
    fm = ui.formatter("bookmarks", opts)
    for bmark, n in sorted(pycompat.iteritems(remotebookmarks)):
        fm.startitem()
        if not ui.quiet:
            fm.plain("   ")
        fm.write("bookmark", "%s", bmark)
        pad = " " * (25 - encoding.colwidth(bmark))
        fm.condwrite(not ui.quiet, "node", pad + " %s", n)
        fm.plain("\n")
    fm.end()


def _http_bookmark_fetch(repo, names) -> sortdict:
    bookmarks = repo.edenapi.bookmarks(names)
    return util.sortdict(((bm, n) for (bm, n) in bookmarks.items() if n is not None))


def _deleteremotebookmarks(ui, repo, path, names) -> None:
    """Prune remote names by removing the bookmarks we don't want anymore,
    then writing the result back to disk
    """
    remotenamesext = extensions.find("remotenames")

    # remotename format is:
    # (node, nametype ("bookmarks"), remote, name)
    nametype_idx = 1
    remote_idx = 2
    name_idx = 3
    remotenames = [
        remotename
        for remotename in remotenamesext.readremotenames(repo)
        if remotename[remote_idx] == path
    ]
    remote_bm_names = [
        remotename[name_idx]
        for remotename in remotenames
        if remotename[nametype_idx] == "bookmarks"
    ]

    for name in names:
        if name not in remote_bm_names:
            raise error.Abort(
                _("scratch bookmark '{}' does not exist " "in path '{}'").format(
                    name, path
                )
            )

    bookmarks = {}
    for node, nametype, remote, name in remotenames:
        if nametype == "bookmarks" and name not in names:
            bookmarks[name] = node

    remotenamesext.saveremotenames(repo, {path: bookmarks})
