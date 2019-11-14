# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import contextlib

from ..i18n import _
from .cmdtable import command


@command("doctor")
def doctor(ui, repo, **opts):
    """attempt to check and fix issues

    This command is still in its early days. So expect it does not fix all
    issues.
    """
    if "remotefilelog" in repo.requirements:
        from ...hgext.remotefilelog import shallowutil
        from bindings import revisionstore

        if repo.ui.configbool("remotefilelog", "indexedlogdatastore"):
            with attempt(ui, "indexedlogdatastore"):
                path = shallowutil.getindexedlogdatastorepath(repo)
                message = revisionstore.indexedlogdatastore.repair(path)
                ui.write_err(indent(message))

        if repo.ui.configbool("remotefilelog", "indexedloghistorystore"):
            with attempt(ui, "indexedloghistorystore"):
                path = shallowutil.getindexedloghistorystorepath(repo)
                message = revisionstore.indexedloghistorystore.repair(path)
                ui.write_err(indent(message))


@contextlib.contextmanager
def attempt(ui, name):
    ui.status(_("attempt to check and fix %s ...\n") % name)
    try:
        yield
    except Exception as ex:
        ui.write_err(_("failed to fix %s: %s\n") % (name, indent(str(ex))))


def indent(message):
    return "".join(l and ("  %s" % l) or "\n" for l in message.splitlines(True))
