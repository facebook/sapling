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

from .. import progress, util
from ..i18n import _
from .cmdtable import command


@command("doctor")
def doctor(ui, repo, **opts):
    # type: (...) -> None
    """attempt to check and fix issues

    This command is still in its early days. So expect it does not fix all
    issues.
    """

    if repo.ui.configbool("mutation", "enabled"):
        # pyre-fixme[18]
        repairsvfs(repo, "mutation", mutationstore.mutationstore)

    if repo.svfs.isdir("metalog"):
        repairsvfs(repo, "metalog", metalog.metalog)

    if repo.svfs.isdir("allheads"):
        repairsvfs(repo, "allheads", nodemap.nodeset)

    if "remotefilelog" in repo.requirements:
        from ...hgext.remotefilelog import shallowutil

        if repo.ui.configbool("remotefilelog", "indexedlogdatastore"):
            path = shallowutil.getindexedlogdatastorepath(repo)
            repair(
                ui,
                "indexedlogdatastore",
                path,
                revisionstore.indexedlogdatastore.repair,
            )

        if repo.ui.configbool("remotefilelog", "indexedloghistorystore"):
            path = shallowutil.getindexedloghistorystorepath(repo)
            repair(
                ui,
                "indexedloghistorystore",
                path,
                revisionstore.indexedloghistorystore.repair,
            )


def repairsvfs(repo, name, fixobj):
    # type: (..., str, ...) -> None
    """Attempt to repair path in repo.svfs"""
    repair(repo.ui, name, repo.svfs.join(name), fixobj.repair)


def repair(ui, name, path, fixfunc):
    # type: (..., str, str) -> None
    """Attempt to repair path by using fixfunc"""
    with progress.spinner(ui, "checking %s" % name):
        oldmtime = mtime(path)
        try:
            message = fixfunc(path)
        except Exception as ex:
            ui.warn(_("%s: failed to fix: %s\n") % (name, ex))
        else:
            newmtime = mtime(path)
            # pyre-fixme[18]
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
