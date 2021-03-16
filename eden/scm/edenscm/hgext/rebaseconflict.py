# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# rebase.py - rebasing feature for mercurial
#
# Copyright 2008 Stefano Tortarolo <stefano.tortarolo at gmail dot com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""rebase with check-in conflicts

Idea comes from [Jujube](https://github.com/martinvonz/jj).
"""

from __future__ import absolute_import

import base64
import errno
import os

import bindings
from edenscm.mercurial import (
    context,
    error,
    mutation,
    registrar,
    scmutil,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import nullid, nullrev, short, wdirid
from edenscm.mercurial.pycompat import encodeutf8, decodeutf8

CommitConflict = bindings.conflict.CommitConflict
FileConflict = bindings.conflict.FileConflict
FileContext = bindings.conflict.FileContext


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "debugrebaseconflict",
    [
        ("r", "rev", [], _("rebase these revisions"), _("REV")),
        ("d", "dest", "", _("rebase onto the specified changeset"), _("REV")),
    ],
)
def rebase(ui, repo, **opts):
    """move commits from one location to another with conflicts checked in

    Currently this is VERY EXPERIMENTAL! It should only be tested by the source
    control team members.
    """
    srcrevs = scmutil.revrange(repo, opts["rev"])
    destrev = scmutil.revsingle(repo, opts["dest"])
    if len(srcrevs) > 1:
        # Multi-rev support can be added later.
        raise error.Abort(_("only 1 rev is supported right now"))
    src = repo[srcrevs.min()]
    dst = repo[destrev]
    mctx = rebaseone(src, dst)
    with repo.lock(), repo.transaction("rebaseconflict"):
        newnode = repo.commitctx(mctx)
        scmutil.cleanupnodes(repo, {src.node(): [newnode]}, "rebase")


@command(
    "debugshowconflict",
    [
        ("r", "rev", [], _("show these revisions"), _("REV")),
    ],
)
def debugshowconflict(ui, repo, **opts):
    """show checked-in conflicts"""
    srcrevs = scmutil.revrange(repo, opts["rev"])
    for rev in srcrevs:
        ctx = repo[rev]
        commitconflict = extractcommitconflict(ctx)
        if commitconflict:
            paths = sorted(commitconflict.toobject()["files"])
            ui.write(_("%s: %d conflicts\n" % (ctx, len(paths))))
            for path in sorted(paths):
                fileconflict = commitconflict.get(path)
                obj = fileconflict.toobject()
                addsdesc = [short(a["commit"] or wdirid) for a in obj["adds"]]
                removesdesc = [short(a["commit"] or wdirid) for a in obj["removes"]]
                ui.write(_("  %s: adds=%s removes=%s\n" % (path, ",".join(addsdesc), ",".join(removesdesc))))
        else:
            ui.write(_("%s: no conflict\n" % ctx))


def rebaseone(srcctx, dstctx):
    """Rebase one commit, using check-in conflicts. Returns a committable context"""
    if not srcctx.mutable():
        raise error.Abort(_("commit %s is immutable") % srcctx)
    basectx = srcctx.p1()
    repo = srcctx.repo()
    diff = srcctx.manifest().diff(dstctx.manifest())
    conflicts = []
    resolved = {}

    for path, ((srcid, srcflag), (dstid, dstflag)) in diff.items():
        basefileconflict = extractfileconflict(basectx, path)
        localfileconflict = extractfileconflict(srcctx, path)
        otherfileconflict = extractfileconflict(dstctx, path)
        # Merge!
        fileconflict = localfileconflict + otherfileconflict - basefileconflict
        if fileconflict.isresolved():
            resolved[path] = fileconflict.adds()[0]
        else:
            # XXX: Try to do some 3-way merges?
            # Not resolved. Put in the conflicts list
            conflicts.append((path, fileconflict))

    extra = {}
    if conflicts:
        commitconflict = CommitConflict(conflicts)
        extra["conflict"] = encodeconflictforextra(commitconflict)
    mutinfo = mutation.record(repo, extra, [srcctx.node()], "rebase")
    loginfo = {"predecessors": srcctx.hex(), "mutation": "rebase"}
    mctx = context.memctx(
        repo,
        parents=(dstctx.node(), nullid),
        text=srcctx.description(),
        files=resolved.keys(),
        filectxfn=dstctx,
        user=srcctx.user(),
        extra=extra,
        mutinfo=mutinfo,
        loginfo=loginfo,
    )
    for path, resolved in resolved.items():
        mctx[path] = fctxfromrust(repo, path, resolved)

    # Try to render the conflicts somehow?
    return mctx


# {node: CommitConflict | None}
_conflictcache = {}


def extractfileconflict(ctx, path):
    """Extract the Rust FileConflict from (ctx, path).

    The Rust FileConflict can be in a conflict state (extracted from commit extra),
    or a non-conflicted state (converted from Python fctx).
    """
    # Parse the commit conflict
    if ctx.node() not in _conflictcache:
        _conflictcache[ctx.node()] = extractcommitconflict(ctx)
    commitconflict = _conflictcache[ctx.node()]

    if commitconflict:
        fileconflict = commitconflict.get(path)
        if fileconflict is not None:
            # Has a check-in conflict for the file
            return fileconflict

    # Build conflict from file context.
    if path in ctx:
        fctx = ctx[path]
        renamed = fctx.renamed()
        renamed = renamed and renamed[0] or None
        filecontext = FileContext(fctx.filenode(), fctx.flags(), renamed, ctx.node())
    else:
        filecontext = FileContext(None, "", None, ctx.node())
    return FileConflict.fromfile(filecontext)


def fctxfromrust(repo, path, rustfctx):
    """Convert a Rust FileContext to a memfilectx

    Returns None if the Rust FileContext means deletion (Python file context
    does not have a matching concept).
    """
    # ex: {'commit': None, 'copy_from': None, 'flags': '', 'id': 'aaaaaaaaaaaaaaaaaaaa'}
    obj = rustfctx.toobject()
    if not obj["id"]:
        # File was deleted. Still keep ctx.node().
        return None
    ctx = repo[obj["commit"]]
    flags = obj["flags"]
    data = readblob(repo, obj["id"])
    renamed = obj["copy_from"]
    if renamed:
        renamed = (renamed, nullid)
    return context.memfilectx(
        repo, ctx, path, data, "l" in flags, "x" in flags, renamed
    )


def extractcommitconflict(ctx):
    """Extract the CommitConflict state stored in the commit"""
    return decodeconflictfromextra(ctx.extra().get("conflict"))


def encodeconflictforextra(commitconflict):
    return decodeutf8(base64.encodestring(commitconflict.tobytes()))


def decodeconflictfromextra(extra):
    if not extra:
        return None
    return CommitConflict.frombytes(base64.decodestring(encodeutf8(extra)))


def readblob(repo, id):
    """Read file content from repo storage using the identify"""
    # This assumes a non-revlog storage that does not need "path" to do
    # lookups.
    return repo.fileslog.contentstore.get("", id)


def writeblob(repo, id, data):
    """Make readblob(id) return data"""
    repo.fileslog.contentstore.add("", id, nullid, data, None)
