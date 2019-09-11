# snapshot - working copy snapshots
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

Configs::

    [ui]
    # Allow to run `hg checkout` for snapshot revisions
    allow-checkout-snapshot = False

    [snapshot]
    # Sync snapshot metadata via bundle2
    enable-sync-bundle = False

    # The local directory to store blob file for sharing across local clones
    # If not set, the cache is disabled (default)
    usercache = /path/to/global/cache
"""

from edenscm.mercurial import error, extensions, hg, registrar
from edenscm.mercurial.i18n import _

from . import blobstore, bundleparts, cmds as snapshotcommands, metadata


cmdtable = snapshotcommands.cmdtable

configtable = {}
configitem = registrar.configitem(configtable)
configitem("ui", "allow-checkout-snapshot", default=False)
configitem("snapshot", "enable-sync-bundle", default=False)
configitem("snapshot", "usercache", default=None)


def uisetup(ui):
    bundleparts.uisetup(ui)


def reposetup(ui, repo):
    # Nothing to do with a remote repo
    if not repo.local():
        return

    repo.svfs.snapshotstore = blobstore.local(repo)


def extsetup(ui):
    extensions.wrapfunction(hg, "updaterepo", _updaterepo)


def _updaterepo(orig, repo, node, overwrite, **opts):
    allowsnapshots = repo.ui.configbool("ui", "allow-checkout-snapshot")
    unfi = repo.unfiltered()
    if not allowsnapshots and node in unfi:
        ctx = unfi[node]
        if "snapshotmetadataid" in ctx.extra():
            raise error.Abort(
                _(
                    "%s is a snapshot, set ui.allow-checkout-snapshot"
                    " config to True to checkout on it\n"
                )
                % ctx
            )
    return orig(repo, node, overwrite, **opts)
