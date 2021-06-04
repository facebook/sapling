# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import node as nodemod, error
from edenscm.mercurial.i18n import _, _n

from . import util as ccutil


def lookupcommits(repo, nodes):
    """Returns list of missing commits"""
    try:
        stream, _stats = repo.edenapi.lookup_commits(ccutil.getreponame(repo), nodes)
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)

    founditems = set()
    for item in stream:
        if item["token"]:
            founditems.add(item["index"])

    return [node for index, node in enumerate(nodes) if index not in founditems]


def lookupfilenodes(repo, filenodes):
    """Returns list of missing filenodes"""
    try:
        stream, _stats = repo.edenapi.lookup_filenodes(
            ccutil.getreponame(repo), filenodes
        )
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)

    founditems = set()
    for item in stream:
        if item["token"]:
            founditems.add(item["index"])

    return [fnode for index, fnode in enumerate(filenodes) if index not in founditems]


def uploadblobs(repo, nodes):
    toupload = set()
    for node in nodes:
        ctx = repo[node]
        for f in ctx.files():
            if f not in ctx:
                continue
            fctx = ctx[f]
            toupload.add(fctx.filenode())
    return toupload


def upload(repo, revs):
    """upload commits to commit cloud using EdenApi

    Commits that have already been uploaded will be skipped.

    The upload will be performed in stages:
        * file content
        * file nodes
        * trees
        * changesets

    If no revision is specified, uploads all visible commits.
    """
    ui = repo.ui

    if revs is None:
        heads = [ctx.node() for ctx in repo.set("heads(not public())")]
    else:
        heads = [
            ctx.node()
            for ctx in repo.set(
                "heads((not public() & ::%ld))",
                revs,
            )
        ]
    if not heads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        return

    # Check what heads have been already uploaded and what heads are missing
    missingheads = lookupcommits(repo, heads)

    if not missingheads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        return

    # Print the heads missing on the server
    _maxoutput = 20
    for counter, node in enumerate(missingheads):
        if counter == _maxoutput:
            left = len(missingheads) - counter
            repo.ui.status(
                _n(
                    "  and %d more head...\n",
                    "  and %d more heads...\n",
                    left,
                )
                % left
            )
            break
        ui.status(
            _("head %s haven't been uploaded yet\n") % nodemod.hex(node)[:12],
            component="commitcloud",
        )

    # Build a queue of commits to upload for the set of missing heads.
    draftrevs = repo.changelog.torevset(
        repo.dageval(lambda: ancestors(missingheads) & draft())
    )
    draftnodes = [repo[r].node() for r in draftrevs]

    uploadcommitqueue = lookupcommits(repo, draftnodes)
    repo.ui.status(
        _n(
            "queue %d commit for upload\n",
            "queue %d commits for upload\n",
            len(uploadcommitqueue),
        )
        % len(uploadcommitqueue),
        component="commitcloud",
    )

    # Build a queue of filenodes to upload
    uploadblobqueue = lookupfilenodes(repo, list(uploadblobs(repo, uploadcommitqueue)))
    repo.ui.status(
        _n(
            "queue %d blob for upload\n",
            "queue %d blobs for upload\n",
            len(uploadblobqueue),
        )
        % len(uploadblobqueue),
        component="commitcloud",
    )

    # TODO (liubovd): implement upload
