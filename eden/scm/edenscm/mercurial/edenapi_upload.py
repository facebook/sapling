# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os

import bindings

from . import node as nodemod, error, mutation
from .i18n import _, _n

TOKEN_KEY = "token"
INDEX_KEY = "index"


def getreponame(repo):
    """get the configured reponame for this repo"""
    reponame = repo.ui.config(
        "remotefilelog",
        "reponame",
        os.path.basename(repo.ui.config("paths", "default")),
    )
    if not reponame:
        raise error.Abort(repo.ui, _("unknown repo"))
    return reponame


def _filtercommits(repo, nodes):
    """Returns list of missing commits"""
    try:
        stream, _stats = repo.edenapi.commitknown(getreponame(repo), nodes)
        return [item["hgid"] for item in stream if item["known"].get("Ok") is not True]
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def _filterfilenodes(repo, keys):
    """Returns list of missing filenodes"""
    try:
        stream, _stats = repo.edenapi.lookup_filenodes(
            getreponame(repo), [key[1] for key in keys]
        )
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)

    return [fnode for index, fnode in enumerate(keys) if index not in foundindices]


def _filtertrees(repo, keys):
    """Returns list of missing trees"""
    try:
        stream, _stats = repo.edenapi.lookup_trees(
            getreponame(repo), [key[0] for key in keys]
        )
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)

    return [tree for index, tree in enumerate(keys) if index not in foundindices]


def _uploadfilenodes(repo, keys):
    """Upload file content and filenodes"""
    if not keys:
        return
    dpack, _hpack = repo.fileslog.getmutablelocalpacks()
    try:
        stream, _stats = repo.edenapi.uploadfiles(dpack, getreponame(repo), keys)
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
        repo.ui.status(
            _n(
                "uploaded %d file\n",
                "uploaded %d files\n",
                len(foundindices),
            )
            % len(foundindices),
            component="edenapi",
        )

        return foundindices

    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def _uploadtrees(repo, trees):
    """Upload trees"""
    if not trees:
        return
    try:
        stream, _stats = repo.edenapi.uploadtrees(getreponame(repo), trees)
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
        repo.ui.status(
            _n(
                "uploaded %d tree\n",
                "uploaded %d trees\n",
                len(foundindices),
            )
            % len(foundindices),
            component="edenapi",
        )
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def _uploadchangesets(repo, changesets, mutations):
    """Upload changesets"""
    uploaded, failed = [], []
    if not changesets:
        return uploaded, failed
    try:
        stream, _stats = repo.edenapi.uploadchangesets(
            getreponame(repo), changesets, mutations
        )
        foundindices = {item[INDEX_KEY] for item in stream if item[TOKEN_KEY]}
        repo.ui.status(
            _n(
                "uploaded %d changeset\n",
                "uploaded %d changesets\n",
                len(foundindices),
            )
            % len(foundindices),
            component="edenapi",
        )
        for index, cs in enumerate(changesets):
            if index in foundindices:
                uploaded.append(cs[0])
            else:
                failed.append(cs[0])
        return uploaded, failed
    except (error.RustError, error.HttpError) as e:
        raise error.Abort(e)


def _getblobs(repo, nodes):
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


def _gettrees(repo, nodes):
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


def _torevs(repo, uploadednodes, failednodes):
    """Convert nodes back to revs"""
    return set([repo[node].rev() for node in uploadednodes]), set(
        [repo[node].rev() for node in failednodes]
    )


def filetypefromfile(f):
    if f.isexec():
        return "Executable"
    elif f.islink():
        return "Symlink"
    else:
        return "Regular"


def parentsfromctx(ctx):
    p1 = ctx.p1().node()
    p2 = ctx.p2().node()
    if p1 != nodemod.nullid and p2 != nodemod.nullid:
        return (p1, p2)
    elif p1 != nodemod.nullid:
        return p1
    else:
        return None


def uploadhgchangesets(repo, revs, force=False):
    """Upload list of revs via EdenApi Uploads protocol

    EdenApi Uploads API consists of the following:

        * Endpoint for lookup any type of data (file contents, hg filenodes,  hg treemanifests, hg commits).
        * Endpoint for upload file contents.
        * Endpoint for upload hg filenodes.
        * Endpoint for upload hg treemanifest.
        * Endpoint for upload hg commits & mutation information.

    The upload process is split into several stages:

        * Check and skip commits that have been already uploaded building ``uploadcommitqueue``.
        * Check and skip hg filenodes that have been already uploaded buiding ``uploadblobqueue``.
        * Calculate ContentIds hashes and upload all file contents for the ``uploadblobqueue``
          but skipping already uploaded content ids first (this step also deduplicates content ids
          if they are the same for some filenodes). See edenapi.uploadfiles.
        * Upload hg filenodes (``uploadblobqueue``).
        * Check and skip hg trees that have been already uploaded buiding ``uploadtreesqueue``.
        * Upload hg trees (``uploadtreesqueue``).
        * Finally, upload hg changesets and hg mutation information (``uploadcommitqueue``).

    If ``force`` is True (the default is False) the lookup check isn't performed prior to upload for commits, filenodes and trees.
    It will be still performed for file contents.

    Returns newly uploaded revs and failed revs.
    """

    nodes = [repo[r].node() for r in revs]

    # Build a queue of commits to upload
    uploadcommitqueue = nodes if force else _filtercommits(repo, nodes)
    repo.ui.status(
        _n(
            "queue %d commit for upload\n",
            "queue %d commits for upload\n",
            len(uploadcommitqueue),
        )
        % len(uploadcommitqueue),
        component="edenapi",
    )

    # Sort uploadcommitqueue in topological order (use iterrev() to iterate from parents to children)
    uploadcommitqueue = repo.changelog.dag.sort(uploadcommitqueue)

    # Build a queue of missing filenodes to upload
    blobs = list(_getblobs(repo, uploadcommitqueue))

    uploadblobqueue = blobs if force else _filterfilenodes(repo, blobs)
    repo.ui.status(
        _n(
            "queue %d file for upload\n",
            "queue %d files for upload\n",
            len(uploadblobqueue),
        )
        % len(uploadblobqueue),
        component="edenapi",
    )

    # Upload missing files and filenodes for the selected set of filenodes
    _uploadfilenodes(repo, uploadblobqueue)

    # Build a queue of missing trees to upload
    trees = list(_gettrees(repo, uploadcommitqueue))
    uploadtreesqueue = trees if force else _filtertrees(repo, trees)
    repo.ui.status(
        _n(
            "queue %d tree for upload\n",
            "queue %d trees for upload\n",
            len(uploadtreesqueue),
        )
        % len(uploadtreesqueue),
        component="edenapi",
    )

    # Upload missing trees
    _uploadtrees(repo, uploadtreesqueue)

    # Uploading changesets
    changesets = []
    for node in uploadcommitqueue.iterrev():
        repo.ui.status(
            _("uploading commit '%s'...\n") % nodemod.hex(node), component="edenapi"
        )
        ctx = repo[node]
        extras = [
            {"key": key.encode(), "value": value.encode()}
            for key, value in ctx.extra().items()
            if key != "branch"
        ]
        (time, timezone) = ctx.date()
        changesets.append(
            (
                node,
                {
                    "parents": parentsfromctx(ctx),
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

    return _torevs(repo, *_uploadchangesets(repo, changesets, mutations))
