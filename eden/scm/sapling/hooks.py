# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""predefined hooks"""

import os
import stat
from typing import Optional

import bindings

from . import util
from .i18n import _


def backgroundfsync(ui, repo, hooktype, **kwargs) -> None:
    """run fsync in background

    Example config::

        [hooks]
        postwritecommand.fsync = python:sapling.hooks.backgroundfsync
    """
    if not repo:
        return
    util.spawndetached(util.hgcmd() + ["debugfsync"], cwd=repo.svfs.join(""))


def edenfs_redirect_fixup(ui, repo, hooktype, **kwargs) -> None:
    """run ``edenfsctl redirect fixup``, potentially in background.

    If the `.eden-redirections` file does not exist in the working copy,
    or is empty, run nothing.

    Otherwise, parse the fixup directories, if they exist and look okay,
    run ``edenfsctl redirect fixup`` in background. This reduces overhead
    especially on Windows.

    Otherwise, run in foreground. This is needed for automation that relies
    on ``checkout HASH`` to setup critical repo redirections.
    """
    is_okay = _is_edenfs_redirect_okay(repo)
    if is_okay is None:
        return

    arg0 = ui.config("edenfs", "command") or "edenfsctl"
    args = ui.config("edenfs", "redirect-fixup") or "redirect fixup"
    cmd = f"{arg0} {args}"
    cwd = repo.root

    if is_okay:
        util.spawndetached(cmd, cwd=cwd, shell=True)
    else:
        ui.system(cmd, cwd=cwd)


def _is_edenfs_redirect_okay(repo) -> Optional[bool]:
    """whether the edenfs redirect directories look okay, or None if redirect
    is unnecessary.
    """
    wvfs = repo.wvfs
    redirections = {}

    # Check edenfs-client/src/redirect.rs for the config paths and file format.
    paths = [".eden-redirections", ".eden/client/config.toml"]

    # Workaround missing .eden/client on Windows. This can be removed once the
    # .eden/client symlink exists on Windows.
    if os.name == "nt" and not repo.wvfs.lexists(".eden/client"):
        try:
            text = repo.wread(".eden/config").decode()
            client_path = bindings.toml.loads(text)["Config"]["client"]
            paths.append(os.path.join(client_path, "config.toml"))
        except Exception as e:
            repo.ui.note_err(_("cannot parse .eden/config: %s\n") % (e,))
            return False

    for path in paths:
        try:
            # Cannot use wvfs.tryread as it audits paths and will refuse to
            # read from .eden/.
            full_path = os.path.join(repo.root, path)
            with open(full_path, "r") as f:
                text = f.read()
            parsed = bindings.toml.loads(text)
        except FileNotFoundError:
            continue
        except Exception as e:
            repo.ui.note_err(
                _("cannot parse edenfs redirection from %s: %s\n") % (path, e)
            )
            return False
        new_redirections = parsed.get("redirections")
        if new_redirections:
            redirections.update(new_redirections)

    if not redirections:
        return None

    root_dev = wvfs.stat("").st_dev
    for path, kind in redirections.items():
        # kind is "bind" or "symlink". On Windows, "bind" is not supported.
        is_symlink = kind == "symlink" or os.name == "nt"
        try:
            st = wvfs.lstat(path)
        except FileNotFoundError:
            return False
        if is_symlink:
            if not stat.S_ISLNK(st.st_mode):
                repo.ui.note_err(_("edenfs redirection %r is not a symlink\n") % path)
                return False
        else:
            # Bind mount should have a different st_dev.
            if st.st_dev == root_dev:
                repo.ui.note_err(_("edenfs redirection %r is not a mount\n") % path)
                return False

    return True
