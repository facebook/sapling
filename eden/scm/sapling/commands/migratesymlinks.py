# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List, Set

from bindings import treestate as treestatemod

from .. import cmdutil, edenfs, error, hg, match, progress, scmutil
from ..i18n import _


def changereposymlinkstatus(ui, repo, enable: bool) -> None:
    if edenfs.requirement in repo.requirements:
        raise error.Abort("EdenFS symlink migration is not supported")
    if enable:
        if "windowssymlinks" in repo.requirements:
            raise error.Abort("repo already supports symlinks")
    else:
        if "windowssymlinks" not in repo.requirements:
            raise error.Abort("repo does not support symlinks")
    ui.write(f"{'Enabling' if enable else 'Disabling'} symlinks for the repo...\n")
    beforestatus = repo.status()
    ui.setconfig("experimental", "windows-symlinks.force", enable)
    repo = hg.repository(ui, repo.root)

    with progress.spinner(ui, _("Detecting symlinks")):
        if sparsematch := getattr(repo, "sparsematch", None):
            matcher = sparsematch()
        else:
            matcher = match.match(repo.root, repo.root)
        files = repo[None].manifest().walk(matcher)
        slinks = filterfiles(
            repo,
            files,
            set(beforestatus.modified + beforestatus.added + beforestatus.removed),
            enable,
        )

    with progress.spinner(ui, _("Updating symlink metadata")):
        treestate = repo.dirstate._map._tree
        needcheckflag = getattr(treestatemod, "NEED_CHECK")
        ctx = repo["."]
        for lnk in slinks:
            # Mark the files that are supposed / not supposed to be symlinks as such so that
            # status expects them to be symlinks
            flags, mode, size, mtime, copied = treestate.get(lnk, None)
            flags |= needcheckflag
            mode ^= 0xA000
            if enable:
                size = 0
            else:
                size = ctx[lnk].size()
            treestate.insert(lnk, flags, mode, size, mtime, copied)
        # Flush the dirstate
        repo.dirstate._dirty = True
        repo.dirstate.write(None)

    # Files cannot simply be converted to/from symlinks, as in non-EdenFS getting
    # the proper type (file vs. directory) for the symlink is somewhat tricky.
    # Fortunately, revert can do this for us.
    if slinks:
        parent, p2 = repo.dirstate.parents()
        ctx = scmutil.revsingle(repo, None)
        with progress.spinner(ui, _("Updating symlinks on disk")):
            cmdutil.revert(ui, repo, ctx, (parent, p2), *slinks, no_backup=True)

    scmutil.writerequires(repo.localvfs, repo.requirements)
    ui.write(f"Symlinks {'enabled' if enable else 'disabled'} for the repo\n")


def filterfiles(
    repo, files: List[str], beforestatus: Set[str], slinks: bool
) -> List[str]:
    """Gets a list of files and returns the ones that are supposed / not
    supposed to be symlinks
    """
    mf = repo["."].manifest()
    return [
        f
        for f in files
        if f not in beforestatus
        and "l" in mf.flags(f)
        and repo.wvfs.islink(f) != slinks
    ]
