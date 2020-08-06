# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""clone utilities that aims for Mononoke compatibility"""

from . import bookmarks as bookmod, error, streamclone
from .i18n import _


def shallowclone(source, repo):
    """clone from source into an empty shallow repo"""

    with repo.wlock(), repo.lock(), repo.transaction("clone"):
        if any(
            repo.svfs.tryread(name)
            for name in ["00changelog.i", "bookmarks", "remotenames"]
        ):
            raise error.Abort(_("clone: repo %s is not empty") % repo.root)

        repo.requirements.add("remotefilelog")
        repo._writerequirements()

        # Invalidate the local changelog length metadata.
        repo.svfs.tryunlink("00changelog.len")

        repo.ui.status(_("fetching changelog\n"))
        with repo.conn(source) as conn:
            # Assume the remote server supports streamclone.
            peer = conn.peer
            fp = peer.stream_out(shallow=True)

            l = fp.readline()
            if l.strip() != b"0":
                raise error.ResponseError(
                    _("unexpected response from remote server:"), l
                )

            l = fp.readline()
            try:
                filecount, bytecount = list(map(int, l.split(b" ", 1)))
            except (ValueError, TypeError):
                raise error.ResponseError(
                    _("unexpected response from remote server:"), l
                )

            # Get 00changelog.{i,d}. This does not write bookmarks or remotenames.
            streamclone.consumev1(repo, fp, filecount, bytecount)
            # repo.changelog needs to be reloaded.
            repo.invalidate()
            repo.invalidatechangelog()

        # Fetch selected remote bookmarks.
        repo.ui.status(_("fetching selected remote bookmarks\n"))
        remote = bookmod.remotenameforurl(repo.ui, repo.ui.paths.getpath(source).rawloc)
        assert remote is not None
        repo.pull(
            source, bookmarknames=bookmod.selectivepullbookmarknames(repo, remote)
        )

        # Data migration.
        if "zstorecommitdata" in repo.storerequirements:
            repo._syncrevlogtozstore()
