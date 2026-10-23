# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""clone utilities that aims for Mononoke compatibility"""

import bindings

from . import bookmarks as bookmod, changelog2, error, util
from .i18n import _
from .node import bin, hex


def _streaming_clone_tag(ui):
    tag = ui.config("stream_out_shallow", "tag")
    if not tag and ui.configbool("stream_out_shallow", "auto"):
        names = ui.configlist("remotenames", "selectivepulldefault")
        if names:
            tag = names[0]
    return tag


def _slapi_streaming_clone(repo) -> None:
    """Perform streaming clone using SLAPI/EdenAPI."""
    tag = _streaming_clone_tag(repo.ui)

    store_path = repo.store_path
    repo.ui.status(_("streaming changelog via SLAPI\n"))

    start = util.timer()
    result = bindings.exchange.streaming_clone(repo.edenapi, store_path, tag)

    index_bytes = result.get("index_bytes_written", 0)
    data_bytes = result.get("data_bytes_written", 0)
    total_bytes = index_bytes + data_bytes

    elapsed = util.timer() - start
    if elapsed <= 0:
        elapsed = 0.001
    repo.ui.status(
        _("transferred %s in %.1f seconds (%s/sec)\n")
        % (util.bytecount(total_bytes), elapsed, util.bytecount(total_bytes / elapsed))
    )

    # repo.changelog needs to be reloaded.
    repo.invalidate()
    repo.invalidatechangelog()


def revlogclone(source, repo) -> None:
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

        if repo.nullableedenapi is None:
            raise error.Abort(_("streaming clone requires a SaplingRemoteAPI server"))
        _slapi_streaming_clone(repo)

        # Fetch selected remote bookmarks.
        repo.ui.status(_("fetching selected remote bookmarks\n"))
        remote = bookmod.remotenameforurl(repo.ui, repo.ui.paths.getpath(source).rawloc)
        assert remote is not None
        repo.pull(
            source, bookmarknames=bookmod.selectivepullbookmarknames(repo, remote)
        )


def emergencyclone(source, repo) -> None:
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
        fetchedbookmarks = repo.edenapi.bookmarks([mainbookmark])
        mainnode = fetchedbookmarks.get(mainbookmark)
        if mainnode is None:
            raise error.RepoLookupError(_("unknown revision %r") % mainbookmark)
        mainnode = bin(mainnode)

        # Write a DAG with one commit.
        cl = changelog2.changelog.opensegments(repo, repo.ui.uiconfig())
        # Pretend the "mainnode" does not have parents.
        cl.inner.addgraphnodes([(mainnode, [])])
        # Write to the "master" group.
        cl.inner.flush([mainnode])

        # Update references
        remote = bookmod.remotenameforurl(repo.ui, repo.ui.paths.getpath(source).rawloc)
        fullname = "%s/%s" % (remote, mainbookmark)
        repo.svfs.write("remotenames", bookmod.encoderemotenames({fullname: mainnode}))
        repo.svfs.write("tip", mainnode)

        repo.ui.status(_("added %s: %s\n") % (mainbookmark, hex(mainnode)))

        repo.invalidate()
        repo.invalidatechangelog()


def segmentsclone(source, repo) -> None:
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
        bindings.exchange.clone(
            repo.ui._rcfg,
            repo.edenapi,
            repo.metalog(),
            repo.changelog.inner,
            bookmarks,
        )
