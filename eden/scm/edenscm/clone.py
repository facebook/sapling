# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""clone utilities that aims for Mononoke compatibility"""

import bindings

from . import bookmarks as bookmod, changelog2, error, streamclone
from .i18n import _
from .node import hex


def revlogclone(source, repo):
    """clone from source into an empty remotefilelog repo using revlog changelog"""

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


def emergencyclone(source, repo):
    """clone only 1 single commit for emergency commit+push use-cases

    The commit graph will be incomplete and there is no way to complete the
    history without reclone. Accessing older commits (ex. checking out an
    older commit, logging file or directory history) is likely broken!

    This can be potentially useful if the server has issues with its commit
    graph components.
    """
    with repo.wlock(), repo.lock(), repo.transaction("emergencyclone"):
        if any(repo.svfs.tryread(name) for name in ["bookmarks", "remotenames", "tip"]):
            raise error.Abort(_("clone: repo %s is not empty") % repo.root)

        repo.requirements.add("remotefilelog")
        repo._writerequirements()

        repo.storerequirements.add("lazytextchangelog")
        repo.storerequirements.add("emergencychangelog")
        repo._writestorerequirements()

        mainbookmark = bookmod.mainbookmark(repo)

        repo.ui.warn(
            _(
                "warning: cloning as emergency commit+push use-case only! "
                "accessing older commits is broken!\n"
            )
        )

        repo.ui.status(_("resolving %s\n") % mainbookmark)
        with repo.conn(source) as conn:
            peer = conn.peer
            mainnode = peer.lookup(mainbookmark)

            # Write a DAG with one commit.
            cl = changelog2.changelog.opensegments(repo, repo.ui.uiconfig())
            # Pretend the "mainnode" does not have parents.
            cl.inner.addgraphnodes([(mainnode, [])])
            # Write to the "master" group.
            cl.inner.flush([mainnode])

            # Update references
            remote = bookmod.remotenameforurl(
                repo.ui, repo.ui.paths.getpath(source).rawloc
            )
            fullname = "%s/%s" % (remote, mainbookmark)
            repo.svfs.write(
                "remotenames", bookmod.encoderemotenames({fullname: mainnode})
            )
            repo.svfs.write("tip", mainnode)

            repo.ui.status(_("added %s: %s\n") % (mainbookmark, hex(mainnode)))

            repo.invalidate()
            repo.invalidatechangelog()


def segmentsclone(source, repo):
    """clone using segmented changelog's CloneData

    This produces a repo with lazy commit hashes.
    """
    with repo.wlock(), repo.lock(), repo.transaction("clone"):
        changelog2.migrateto(repo, "lazy")
        repo.requirements.add("remotefilelog")
        repo._writerequirements()

        remote = bookmod.remotenameforurl(repo.ui, repo.ui.paths.getpath(source).rawloc)
        bookmarks = bookmod.selectivepullbookmarknames(repo, remote)

        repo.ui.status(_("populating main commit graph\n"))
        if repo.ui.configbool("clone", "nativepull"):
            bindings.exchange.clone(
                repo.edenapi, repo.metalog(), repo.changelog.inner, bookmarks
            )
        else:
            clonedata = repo.edenapi.clonedata()
            repo.changelog.inner.importclonedata(clonedata)
            tip = repo.changelog.dag.all().first()
            if tip:
                repo.ui.status(_("tip commit: %s\n") % hex(tip))
                repo.svfs.write("tip", tip)

            repo.ui.status(_("fetching selected remote bookmarks\n"))
            assert remote is not None
            repo.pull(source, bookmarknames=bookmarks)
