# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# dirsync.py - keep two directories synchronized at commit time
"""
keep directories in a repo synchronized (DEPRECATED)

Configure it by adding the following config options to your .hg/hgrc or
.hgdirsync in the root of the repo::

    [dirsync]
    projectX.dir1 = path/to/dir1
    projectX.dir2 = path/dir2

The configs are of the form "group.name = path-to-dir". Every config entry with
the same `group` will be mirrored amongst each other. The `name` is just used to
separate them and is not used anywhere. The `path` is the path to the directory
from the repo root. It must be a directory, but it doesn't matter if you specify
the trailing '/' or not.

Multiple mirror groups can be specified at once, and you can mirror between an
arbitrary number of directories::

    [dirsync]
    projectX.dir1 = path/to/dir1
    projectX.dir2 = path/dir2
    projectY.dir1 = otherpath/dir1
    projectY.dir2 = foo/bar
    projectY.dir3 = foo/goo/hoo

If you wish to exclude a subdirectory from being synced, for every rule in a group,
create a rule for the subdirectory and prefix it with "exclude":

        [dirsync]
        projectX.dir1 = dir1/foo
        exclude.projectX.dir1 = dir1/foo/bar
        projectX.dir2 = dir2/dir1/foo
        exclude.projectX.dir2 = dir2/dir1/foo/bar
"""

from __future__ import absolute_import

import bindings
from edenscm import tracing
from edenscm.mercurial import (
    config,
    context,
    error,
    extensions,
    localrepo,
    pycompat,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin


testedwith = "ships-with-fb-hgext"
EXCLUDE_PATHS = "exclude"

_disabled = [False]
_nodemirrored = {}  # {node: {path}}, for syncing from commit to wvfs


def extsetup(ui):
    extensions.wrapfunction(localrepo.localrepository, "commitctx", _commitctx)

    def wrapshelve(loaded=False):
        try:
            shelvemod = extensions.find("shelve")
            extensions.wrapcommand(shelvemod.cmdtable, "shelve", _bypassdirsync)
            extensions.wrapcommand(shelvemod.cmdtable, "unshelve", _bypassdirsync)
        except KeyError:
            pass

    extensions.afterloaded("shelve", wrapshelve)


def reposetup(ui, repo):
    if not repo.local() or not util.safehasattr(repo, "dirstate"):
        return

    # If dirstate is updated to a commit that has 'mirrored' paths without
    # going though regular checkout code path, write the mirrored files to disk
    # and mark them as clean (or remove mirrored deletions).
    #
    # Note: This is not needed for a regular checkout that updates dirstate,
    # but there are code paths (ex. reset, amend, absorb) that updates
    # dirstate parents directly for performance reasons. This fixup is
    # necessary for them, because dirsync breaks their assumptions about what
    # files have changed (dirsync introduces more changed files without their
    # consent).
    #
    # Similar to run `hg revert <mirrored paths>`.

    def dirsyncfixup(dirstate, old, new, repo=repo):
        p1, p2 = new
        mirrored = _nodemirrored.get(p1, None)
        if not mirrored:
            return
        paths = sorted(mirrored)
        wctx = repo[None]
        ctx = repo[p1]
        for path in paths:
            wf = wctx[path]
            if path in ctx:
                # Modified.
                f = ctx[path]
                wf.write(f.data(), f.flags())
                dirstate.normal(path)
                tracing.debug("rewrite mirrored %s" % path)
            else:
                # Deleted.
                if wf.exists():
                    wf.remove()
                    dirstate.delete(path)
                    tracing.debug("remove mirrored %s" % path)
        # The working copy is in sync. No need to fixup again.
        _nodemirrored.clear()

    repo.dirstate.addparentchangecallback("dirsync", dirsyncfixup)


def _bypassdirsync(orig, ui, repo, *args, **kwargs):
    _disabled[0] = True
    try:
        return orig(ui, repo, *args, **kwargs)
    finally:
        _disabled[0] = False


def getconfigs(wctx):
    """returns {name: [path]}.
    [path] under a same name are synced. name is not useful.
    """
    # read from .hgdirsync in repo
    filename = ".hgdirsync"
    try:
        content = pycompat.decodeutf8(wctx[filename].data())
    except (error.ManifestLookupError, IOError, AttributeError, KeyError):
        content = ""
    cfg = config.config()
    if content:
        cfg.parse(filename, "[dirsync]\n%s" % content, ["dirsync"])

    maps = util.sortdict()
    repo = wctx.repo()
    for key, value in repo.ui.configitems("dirsync") + cfg.items("dirsync"):
        if "." not in key:
            continue
        name, disambig = key.split(".", 1)
        # Normalize paths to have / at the end. For easy concatenation later.
        if value[-1] != "/":
            value = value + "/"
        if name not in maps:
            maps[name] = []
        maps[name].append(value)
    return maps


def configstomatcher(configs):
    """returns a matcher matching files need to be dirsynced

    configs is the return value of getconfigs()
    """
    rules = set()
    for mirrors in configs.values():
        for mirror in mirrors:
            assert mirror.endswith("/"), "getconfigs() ensures this"
            rules.add("%s**" % mirror)
    m = bindings.pathmatcher.treematcher(sorted(rules))
    return m


def getmirrors(maps, filename):
    """Returns (srcmirror, mirrors)

    srcmirror is the mirror path the filename is in.
    mirrors is a list of mirror paths, including srcmirror.
    """
    # The getconfigs() code above adds "/" to the end of all of the entries.
    # This makes it easy to check that we are only matching full directory path name
    # components (e.g., so we don't incorrectly treat "foobar/test.txt" as matching a
    # rule for "foo").
    #
    # However, we do want to allow rules to match exact file names too, and not just
    # directory prefixes.  Therefore when looking matches append "/" to the end of the
    # filename that we are checking.
    checkpath = filename + "/"

    if EXCLUDE_PATHS in maps:
        for subdir in maps[EXCLUDE_PATHS]:
            if checkpath.startswith(subdir):
                return None, []

    for key, mirrordirs in maps.items():
        for subdir in mirrordirs:
            if checkpath.startswith(subdir):
                return subdir, mirrordirs

    return None, []


def _mctxstatus(ctx, matcher=None):
    """Figure out what has changed that need to be synced

    Return (mctx, status).
        - mctx: mirrored ctx
        - status: 'scmutil.status' struct for changes that need to be
          considered for dirsync.

    There are different cases. For example:

    Commit: compare with p1:

        o ctx
        |
        o ctx.p1

    Rebase (and other parent-change mutations): compare with p1 (re-sync),
    because comparing with pred can be slow due to potential long distance

        o pred  ----->   o ctx
        |                |
        o pred.p1        o ctx.p1

    Amend (and other content-change mutations): compare with pred, because
    comparing with p1 might cause sync conflicts:

        o ctx
        |
        | o pred
        |/
        o pred.p1, ctx.p1

        Conflict example: dirsync dir1/ dir2/

            ctx.p1:
                dir1/A = 1
                dir2/A = 1
            pred:
                dir1/A = 2
                dir2/A = 2
            ctx:
                dir1/A = 3
                dir2/A = 2 <- should be considered as "unchanged"

    Stack amend (ex. absorb): compare with pred to avoid conflicts and pick up
    needed changes:

        o ctx
        |
        o ctx.p1
        |
        | o pred(ctx)
        | |
        | o pred(ctx.p1), pred(ctx).p1
        |/
        o ctx.p1.p1, pred(ctx.p1).p1, pred(ctx).p1.p1

    """
    repo = ctx.repo()
    mctx = context.memctx.mirror(ctx)
    mutinfo = ctx.mutinfo()

    # mutpred looks like:
    # hg/2dc0850429134cc0d21d84d6e5a3960faa9aadce,hg/ccfb9effa811b10fdc40e314524f9340c31085d7
    # The first one is the "top" of the stack that we care about.
    mutpredhex = mutinfo and mutinfo["mutpred"].split(",", 1)[0].lstrip("hg/")
    mutpred = mutpredhex and bin(mutpredhex)
    predctx = mutpred and mutpred in repo and repo[mutpred] or None

    # By default, consider all changes in this commit as needing syncing.
    status = mctx._status

    # But, if there was an amend/absorb predecessor commit, remove any changes
    # that are not different from the predecessor (i.e. things not changed in
    # the amend/absorb) and add any removes that happened since the predecessor.
    if predctx:
        if predctx.p1() == ctx.p1() or ctx.p1().node() in _nodemirrored:
            status = _adjuststatus(status, predctx, ctx)

    return mctx, status


def dirsyncctx(ctx, matcher=None):
    """for changes in ctx that matches matcher, apply dirsync rules

    Return:

        (newctx, {path})

    This function does not change working copy or dirstate.
    """
    maps = getconfigs(ctx)
    resultmirrored = set()
    resultctx = ctx

    # Do not dirsync if there is nothing to sync.
    # Do not dirsync metaedit commits, because they might break assertions in
    # metadataonlyctx (manifest is unchanged).
    if not maps or (ctx.mutinfo() or {}).get("mutop") == "metaedit":
        return resultctx, resultmirrored

    needsync = configstomatcher(maps)
    repo = ctx.repo()
    mctx, status = _mctxstatus(ctx)

    added = set(status.added)
    modified = set(status.modified)
    removed = set(status.removed)

    if matcher is None:
        matcher = lambda path: True

    for action, paths in (
        ("a", status.added),
        ("m", status.modified),
        ("r", status.removed),
    ):
        for src in paths:
            if not needsync.matches(src) or not matcher(src):
                continue
            srcmirror, mirrors = getmirrors(maps, src)
            if not mirrors:
                continue

            dstpaths = []  # [(dstpath, dstmirror)]
            for dstmirror in (m for m in mirrors if m != srcmirror):
                dst = _mirrorpath(srcmirror, dstmirror, src)
                dstpaths.append((dst, dstmirror))

            if action == "r":
                fsrc = None
            else:
                fsrc = ctx[src]
            for dst, dstmirror in dstpaths:
                # changed: whether ctx[dst] is changed, according to status.
                # conflict: whether the dst change conflicts with src change.
                if dst in removed:
                    conflict, changed = (action != "r"), True
                elif dst in modified or dst in added:
                    conflict, changed = (fsrc is None or ctx[dst].cmp(fsrc)), True
                else:
                    conflict = changed = False
                if conflict:
                    raise error.Abort(
                        _(
                            "path '%s' needs to be mirrored to '%s', but "
                            "the target already has pending changes"
                        )
                        % (src, dst)
                    )
                if changed:
                    if action == "r":
                        fmt = _(
                            "not mirroring remove of '%s' to '%s'; it is already removed\n"
                        )
                    else:
                        fmt = _("not mirroring '%s' to '%s'; it already matches\n")
                    repo.ui.note(fmt % (src, dst))
                    continue

                # Mirror copyfrom, too.
                renamed = fsrc and fsrc.renamed()
                fmirror = fsrc
                msg = None
                if renamed:
                    copyfrom, copynode = renamed
                    newcopyfrom = _mirrorpath(srcmirror, dstmirror, copyfrom)
                    if newcopyfrom:
                        if action == "a":
                            msg = _("mirrored copy '%s -> %s' to '%s -> %s'\n") % (
                                copyfrom,
                                src,
                                newcopyfrom,
                                dst,
                            )
                        fmirror = context.overlayfilectx(
                            fsrc, copied=(newcopyfrom, copynode)
                        )

                mctx[dst] = fmirror
                resultmirrored.add(dst)

                if msg is None:
                    if action == "a":
                        fmt = _("mirrored adding '%s' to '%s'\n")
                    elif action == "m":
                        fmt = _("mirrored changes in '%s' to '%s'\n")
                    else:
                        fmt = _("mirrored remove of '%s' to '%s'\n")
                    msg = fmt % (src, dst)
                repo.ui.status(msg)

    if resultmirrored:
        resultctx = mctx

    return resultctx, resultmirrored


def _mirrorpath(srcdir, dstdir, src):
    """Mirror src path from srcdir to dstdir. Return None if src is not in srcdir."""
    if src + "/" == srcdir:
        # special case: src is a file to mirror
        return dstdir.rstrip("/")
    elif src.startswith(srcdir):
        relsrc = src[len(srcdir) :]
        return dstdir + relsrc
    else:
        return None


def _adjuststatus(status, ctx1, ctx2):
    """Adjusts the status result to remove item that don't differ between ctx1
    and ctx2

    `ctx2` is the post-amend, pre-dirsync commit. `ctx1` is the pre-amend
    predecessor commit. `status` is the status for ctx2. So this function
    adjusts status to remove items that didn't change between the predecessor
    and the successor, and adds items that existed in the predecessor but not
    the successor."""
    # Table of possible states for a file between ctx1 and ctx2:
    #  C - clean relative to p1()
    #  X - not present in p1() or in ctx
    #  A - added relative to p1
    #  M - modified relative to p1
    #  = - added/modified but ctx1[file].data() == ctx2[file].data()
    #  R - removed - present in p1() but not in ctx
    #
    #  ctx1  ctx2  action     note
    #   XC    XC    -         The file is unaffected by this commit
    #   XC    AMR   -         The file is already in `status`
    #
    #   A     X     removed   The file was Added before, then was reverted. Add to `status.removed` (case 2)
    #   A     C     modified  The file was Added before, but is now tracked/clean. Add to `status.modified`. (case 3)
    #   A     =     drop      Remove the file from `status.added` as it does not need syncing (case 1)
    #   A     AMR   -         The file is already in `status`
    #
    #   M     X     removed   The file was removed during rebase. Add to `status.removed` (case 2)
    #   M     C     modified  The file was Modified before, but is now clean. Add to `status.modified` (case 3)
    #   M     =     drop      Remove the file from `status.modified` as it does not need syncing (case 1)
    #   M     AMR   -         The file is already in `status`

    #   R     X     -         The file is already removed. No need to dirsync.
    #   R     C     modified  The file was Removed before, but is now clean. Add to `status.modified` (case 3)
    #   R     R     -         Remove the file from `status.removed` as it does not need syncing (case 1)
    #   R     AMR   -         The file is already in `status`

    newmodified = []
    newadded = []
    newremoved = []
    skipped = set()

    # Remove files that haven't changed.
    for oldpaths, newpaths in [
        (status.modified, newmodified),
        (status.added, newadded),
    ]:
        for path in oldpaths:
            # No real changes? (case 1)
            if path in ctx1 and path in ctx2:
                f1 = ctx1[path]
                f2 = ctx2[path]
                if f1.flags() == f2.flags() and not f1.cmp(f2):
                    skipped.add(path)
                    continue
            newpaths.append(path)

    for path in status.removed:
        # No real changes (case 1)
        if path not in ctx1 and path not in ctx2:
            skipped.add(path)
            continue
        newremoved.append(path)

    # Add files that have been reverted.
    for oldpath in ctx1.files():
        if (
            oldpath in ctx1
            and oldpath not in ctx2
            and oldpath not in newremoved
            and oldpath not in skipped
        ):
            # The file was in ctx1, but is no longer in ctx2. Mark it removed. (case 2)
            newremoved.append(oldpath)
        elif (
            oldpath not in newmodified
            and oldpath not in newadded
            and oldpath not in skipped
        ):
            # The file had a change in ctx1, but no longer has a change in ctx2.
            # Mark it as modified, so the revert will be synced to ctx2. (case 3)
            newmodified.append(oldpath)

    return scmutil.status(newmodified, newadded, newremoved, [], [], [], [])


def _commitctx(orig, self, ctx, *args, **kwargs):
    if _disabled[0]:
        return orig(self, ctx, *args, **kwargs)

    ctx, mirrored = dirsyncctx(ctx)
    node = orig(self, ctx, *args, **kwargs)

    if mirrored:
        # used by dirsyncfixup to write back from commit to disk
        _nodemirrored[node] = mirrored

    return node
