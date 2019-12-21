# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import contextlib
import os
import typing

# pyre-fixme[21]
from bindings import metalog, mutationstore, nodemap, revisionstore, tracing

from .. import error, hg, progress, util, vfs as vfsmod
from ..i18n import _
from .cmdtable import command


# This command has to be norepo since loading a repo might just fail.
@command("doctor", norepo=True)
def doctor(ui, **opts):
    # type: (...) -> None
    """attempt to check and fix issues

    This command is still in its early days. So expect it does not fix all
    issues.
    """

    from .. import dispatch  # avoid cycle

    # Minimal logic to get key repo objects without actually constructing
    # a real repo object.
    repopath, ui = dispatch._getlocal(ui, "")
    if not repopath:
        raise error.Abort(_("doctor only works inside a repo"))
    repohgpath = os.path.join(repopath, ".hg")
    vfs = vfsmod.vfs(repohgpath)
    sharedhgpath = vfs.tryread("sharedpath") or repohgpath
    svfs = vfsmod.vfs(os.path.join(sharedhgpath, "store"))

    if ui.configbool("mutation", "enabled"):
        repairsvfs(ui, svfs, "mutation", mutationstore.mutationstore)

    if svfs.isdir("metalog"):
        repairsvfs(ui, svfs, "metalog", metalog.metalog)

    if svfs.isdir("allheads"):
        repairsvfs(ui, svfs, "allheads", nodemap.nodeset)

    # Construct the real repo object as shallowutil requires it.
    repo = hg.repository(ui, repopath)
    if "remotefilelog" in repo.requirements:
        from ...hgext.remotefilelog import shallowutil

        if ui.configbool("remotefilelog", "indexedlogdatastore"):
            path = shallowutil.getindexedlogdatastorepath(repo)
            repair(
                ui,
                "indexedlogdatastore",
                path,
                revisionstore.indexedlogdatastore.repair,
            )

        if ui.configbool("remotefilelog", "indexedloghistorystore"):
            path = shallowutil.getindexedloghistorystorepath(repo)
            repair(
                ui,
                "indexedloghistorystore",
                path,
                revisionstore.indexedloghistorystore.repair,
            )


def repairsvfs(ui, svfs, name, fixobj):
    # type: (..., ..., str, ...) -> None
    """Attempt to repair path in repo.svfs"""
    repair(ui, name, svfs.join(name), fixobj.repair)


def repair(ui, name, path, fixfunc):
    # type: (..., str, str, ...) -> None
    """Attempt to repair path by using fixfunc"""
    with progress.spinner(ui, "checking %s" % name):
        oldmtime = mtime(path)
        try:
            message = fixfunc(path)
        except Exception as ex:
            ui.warn(_("%s: failed to fix: %s\n") % (name, ex))
        else:
            newmtime = mtime(path)
            tracing.singleton.event(
                (("cat", "repair"), ("name", "repair %s" % name), ("details", message))
            )
            if ui.verbose:
                ui.write_err(_("%s:\n %s\n") % (name, indent(message)))
            else:
                if oldmtime != newmtime:
                    ui.write_err(_("%s: repaired\n") % name)
                else:
                    ui.write_err(_("%s: looks okay\n") % name)


def mtime(path):
    # type: (str) -> int
    """Return an integer that is likely changed if content of the directory is changed"""
    mtime = 0
    for dirpath, _dirnames, filenames in os.walk(path):
        paths = [os.path.join(path, dirpath, name) for name in filenames]
        mtime += sum(
            (st.st_mtime % 1024) + st.st_size * 1024
            for st in util.statfiles(paths)
            if st
        )
    return mtime


def indent(message):
    # type: (str) -> str
    return "".join(l and ("  %s" % l) or "\n" for l in message.splitlines(True)) + "\n"
