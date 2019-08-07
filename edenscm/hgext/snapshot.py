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
from collections import defaultdict

from edenscm.mercurial import error, extensions, json, registrar
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

    This is a debug command => it lacks security and is unsuitable for prod.
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
    wctx = repo[None]
    for unknown in stat.unknown:
        data = wctx[unknown].data()
        oid, size = uploadtolfs(repo, data)
        manifest["unknown"][unknown] = {"oid": oid, "size": size}
    # store manifest into local lfs
    oid, size = uploadtolfs(repo, json.dumps(manifest))
    ui.status(_("manifest oid: %s\n") % oid)


@command("debuguploadsnapshotmanifest", [], _("OID"), inferrepo=True)
def debuguploadsnapshotmanifest(ui, repo, *args, **opts):
    """
    Uploads manifest and all related blobs to remote lfs.
    Takes in an oid of the desired manifest in the local lfs.

    This is a debug command => it lacks security and is unsuitable for prod.
    """
    if lfs is None:
        raise error.Abort(_("lfs not initialised"))
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a manifest oid"))
    manifestoid = args[0]
    store = repo.svfs.lfslocalblobstore
    if not store.has(manifestoid):
        raise error.Abort(
            _("manifest oid %s not found in local blobstorage") % manifestoid
        )
    # TODO(alexeyqu): wrap it into manifest class with data validation etc
    manifest = json.loads(store.read(manifestoid))
    # prepare pointers to blobs for uploading into remote lfs
    pointers = [lfs.pointer.gitlfspointer(oid=manifestoid, size=str(len(manifest)))]
    for filename, pointer in manifest["unknown"].items():
        oid = pointer["oid"]
        if not store.has(oid):
            raise error.Abort(
                _("file %s with oid %s not found in local blobstorage")
                % (filename, oid)
            )
        pointers.append(lfs.pointer.gitlfspointer(oid=oid, size=pointer["size"]))
    lfs.wrapper.uploadblobs(repo, pointers)
    ui.status(_("upload complete\n"))
