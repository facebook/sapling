# -*- coding: utf-8 -*-

# snapshot.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""extension to snapshot the working copy

With this extension, Mercurial will get a set of commands
for working with full snapshots of the working copy,
including the untracked files and unresolved merge artifacts.

TODO(alexeyqu): finish docs
"""

import hashlib
import json
from collections import defaultdict

from edenscm.mercurial import error, extensions, registrar
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
lfs = None


def extsetup(ui):
    global lfs
    try:
        lfs = extensions.find("lfs")
    except KeyError:
        ui.warning("snapshot extension requires lfs to be enabled")


def uploadtolfs(repo, data):
    """
    Util function which uploads data to the local lfs storage.
    Returns oid and size of data (TODO move to special class).
    """
    # TODO(alexeyqu): do we care about metadata?
    oid = hashlib.sha256(data).hexdigest()
    repo.svfs.lfslocalblobstore.write(oid, data)
    return oid, str(len(data))


@command("debugcreatesnapshotmanifest", inferrepo=True)
def debugcreatesnapshotmanifest(ui, repo, *args, **opts):
    """
    Creates pseudo manifest for untracked files without committing them.
    Loads untracked files and the created manifest into local lfsstore.
    Outputs the oid of the created manifest file.
    """
    if lfs is None:
        raise error.Abort(_("lfs is not initialised"))
    stat = repo.status(unknown=True)
    if not stat.deleted and not stat.unknown:
        ui.status(
            _(
                "Working copy is even with the last commit. "
                "No need to create snapshot.\n"
            )
        )
        return
    manifest = defaultdict(dict)
    # store missing files
    manifest["deleted"] = {d: None for d in stat.deleted}
    # store untracked files into local lfs
    for unknown in stat.unknown:
        data = repo[None][unknown].data()
        oid, size = uploadtolfs(repo, data)
        manifest["unknown"][unknown] = {"oid": oid, "size": size}
    # store manifest into local lfs
    data = json.dumps(manifest, sort_keys=True)
    oid, size = uploadtolfs(repo, data)
    ui.status(_("manifest oid: %s\n") % oid)
