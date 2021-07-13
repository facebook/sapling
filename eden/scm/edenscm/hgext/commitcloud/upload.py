# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import bindings
from edenscm.mercurial import node as nodemod, error, mutation
from edenscm.mercurial.i18n import _, _n

from . import util as ccutil


TOKEN_KEY = "token"
INDEX_KEY = "index"


def lookupcommits(repo, nodes):
    """Returns list of missing commits"""
    try:
        stream, _stats = repo.edenapi.commitknown(ccutil.getreponame(repo), nodes)
        return [item["hgid"] for item in stream if item["known"].get("Ok") is not True]
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def lookupfilenodes(repo, keys):
    """Returns list of missing filenodes"""
    try:
        stream, _stats = repo.edenapi.lookup_filenodes(
            ccutil.getreponame(repo), [key[1] for key in keys]
        )
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)

    return [fnode for index, fnode in enumerate(keys) if index not in foundindices]


def lookuptrees(repo, keys):
    """Returns list of missing trees"""
    try:
        stream, _stats = repo.edenapi.lookup_trees(
            ccutil.getreponame(repo), [key[0] for key in keys]
        )
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)

    return [tree for index, tree in enumerate(keys) if index not in foundindices]


def uploadfiles(repo, keys):
    """Upload file content and filenodes"""
    if not keys:
        return
    dpack, _hpack = repo.fileslog.getmutablelocalpacks()
    try:
        stream, _stats = repo.edenapi.uploadfiles(dpack, ccutil.getreponame(repo), keys)
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
        repo.ui.status(
            _n(
                "uploaded %d file\n",
                "uploaded %d files\n",
                len(foundindices),
            )
            % len(foundindices),
            component="commitcloud",
        )
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def uploadtrees(repo, trees):
    """Upload trees"""
    if not trees:
        return
    try:
        stream, _stats = repo.edenapi.uploadtrees(ccutil.getreponame(repo), trees)
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
        repo.ui.status(
            _n(
                "uploaded %d tree\n",
                "uploaded %d trees\n",
                len(foundindices),
            )
            % len(foundindices),
            component="commitcloud",
        )
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def uploadchangesets(repo, changesets, mutations):
    """Upload changesets"""
    if not changesets:
        return
    try:
        stream, _stats = repo.edenapi.uploadchangesets(
            ccutil.getreponame(repo), changesets, mutations
        )
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
        repo.ui.status(
            _n(
                "uploaded %d changeset\n",
                "uploaded %d changesets\n",
                len(foundindices),
            )
            % len(foundindices),
            component="commitcloud",
        )
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def getblobs(repo, nodes):
    """Get changed files"""
    toupload = set()
    for node in nodes.iterrev():
        ctx = repo[node]
        for f in ctx.files():
            if f not in ctx:
                continue
            fctx = ctx[f]
            p1, p2 = fctx.filelog().parents(fctx.filenode())
            toupload.add((fctx.path(), fctx.filenode(), p1, p2))
    return toupload


def gettrees(repo, nodes):
    """Get changed trees"""
    treedepth = 1 << 15
    for node in nodes.iterrev():
        parentnodes = repo.changelog.dag.parentnames(node)
        mfnode = repo.changelog.changelogrevision(node).manifest
        basemfnodes = [
            repo.changelog.changelogrevision(p).manifest for p in parentnodes
        ]
        difftrees = bindings.manifest.subdirdiff(
            repo.manifestlog.datastore, "", mfnode, basemfnodes, treedepth
        )
        for subdir, treenode, treetext, _x, _x, _x in difftrees:
            p1, p2, _link, _copy = repo.manifestlog.historystore.getnodeinfo(
                subdir, treenode
            )
            yield treenode, p1, p2, treetext


def upload(repo, revs, force=False):
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
    missingheads = heads if force else lookupcommits(repo, heads)

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
            _("head '%s' hasn't been uploaded yet\n") % nodemod.hex(node)[:12],
            component="commitcloud",
        )

    # Build a queue of commits to upload for the set of missing heads.
    draftrevs = repo.changelog.torevset(
        repo.dageval(lambda: ancestors(missingheads) & draft())
    )
    draftnodes = [repo[r].node() for r in draftrevs]

    uploadcommitqueue = draftnodes if force else lookupcommits(repo, draftnodes)
    repo.ui.status(
        _n(
            "queue %d commit for upload\n",
            "queue %d commits for upload\n",
            len(uploadcommitqueue),
        )
        % len(uploadcommitqueue),
        component="commitcloud",
    )

    # Sort uploadcommitqueue in topological order (use iterrev() to iterate from parents to children)
    uploadcommitqueue = repo.changelog.dag.sort(uploadcommitqueue)

    # Build a queue of missing filenodes to upload
    blobs = list(getblobs(repo, uploadcommitqueue))
    uploadblobqueue = blobs if force else lookupfilenodes(repo, blobs)
    repo.ui.status(
        _n(
            "queue %d file for upload\n",
            "queue %d files for upload\n",
            len(uploadblobqueue),
        )
        % len(uploadblobqueue),
        component="commitcloud",
    )

    # Upload missing files and filenodes for the selected set of filenodes
    uploadfiles(repo, uploadblobqueue)

    # Build a queue of missing trees to upload
    trees = list(gettrees(repo, uploadcommitqueue))
    uploadtreesqueue = trees if force else lookuptrees(repo, trees)
    repo.ui.status(
        _n(
            "queue %d tree for upload\n",
            "queue %d trees for upload\n",
            len(uploadtreesqueue),
        )
        % len(uploadtreesqueue),
        component="commitcloud",
    )

    # Upload missing trees
    uploadtrees(repo, uploadtreesqueue)

    # Uploading changesets
    changesets = []
    for node in uploadcommitqueue.iterrev():
        ui.status(
            _("uploading commit '%s'...\n") % nodemod.hex(node), component="commitcloud"
        )
        ctx = repo[node]
        extras = [
            {"key": key.encode(), "value": value.encode()}
            for key, value in ctx.extra().items()
            if key != "branch"
        ]
        (time, timezone) = ctx.date()
        p1 = ctx.p1().node()
        p2 = ctx.p2().node()
        if p1 != nodemod.nullid and p2 != nodemod.nullid:
            parents = (p1, p2)
        elif p1 != nodemod.nullid:
            parents = p1
        else:
            parents = None
        changesets.append(
            (
                node,
                {
                    "parents": parents,
                    "manifestid": ctx.manifestnode(),
                    "user": ctx.user().encode(),
                    "time": int(time),
                    "tz": timezone,
                    "extras": extras,
                    "files": ctx.files(),
                    "message": ctx.description().encode(),
                },
            )
        )

    mutations = mutation.entriesfornodes(repo, uploadcommitqueue)
    mutations = [
        {
            "successor": mut.succ(),
            "predecessors": mut.preds(),
            "split": mut.split(),
            "op": mut.op(),
            "user": mut.user().encode(),
            "time": mut.time(),
            "tz": mut.tz(),
            "extras": [{"key": key, "value": value} for key, value in mut.extra()],
        }
        for mut in mutations
    ]

    uploadchangesets(repo, changesets, mutations)
