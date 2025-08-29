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
create a rule for the subdirectory and prefix it with "exclude"::

    [dirsync]
    projectX.dir1 = dir1/foo
    exclude.projectX.dir1 = dir1/foo/bar
    projectX.dir2 = dir2/dir1/foo
    exclude.projectX.dir2 = dir2/dir1/foo/bar

If you want to customize the mirror behavior, you can write a script like::

    # dirsync/projectX.py
    def mirror_path(src_dir, dst_dir, src_rel):
        # Optional function to customize mirrored path.
        # Example: src_dir="a/dir1/", dst_dir="a/dir2/", src_rel="c/d.txt"
        # The full path of source file is src_dir + src_rel, "a/dir1/c/d.txt".
        # The mirrored path will be dst_dir + return value.
        # If return value is None, the mirror behavior will be skipped.
        return src_rel

    def mirror_data(src_dir, dst_dir, src_rel, src_data: bytes) -> bytes:
        # Optional function to customize mirrored file content.
        # Similar to mirror_path, but returns the content of the mirrored file.
        return src_data

then specify the script in the ``[dirsync-scripts]`` config section::

    [dirsync-scripts]
    projectX = dirsync/projectX.py

The ``[dirsync]`` configs can also be part of the ``.hgdirsync`` file in the
repo root. The ``[dirsync-scripts]`` in ``.hgdirsync`` is ignored by default,
unless the ``dirsync.allow-in-repo-scripts`` config is enabled.
"""

from typing import Optional

import bindings

from sapling import (
    context,
    error,
    extensions,
    localrepo,
    match as matchmod,
    scmutil,
    tracing,
    util,
)
from sapling.ext import shelve as shelvemod
from sapling.i18n import _
from sapling.node import bin
from sapling.scmutil import status
from sapling.util import sortdict

testedwith = "ships-with-fb-ext"
EXCLUDE_PATHS = "exclude"

_disabled = [False]
_nodemirrored = {}  # {node: {path}}, for syncing from commit to wvfs


def extsetup(ui) -> None:
    extensions.wrapfunction(localrepo.localrepository, "commitctx", _commitctx)

    extensions.wrapcommand(shelvemod.cmdtable, "shelve", _bypassdirsync)
    extensions.wrapcommand(shelvemod.cmdtable, "unshelve", _bypassdirsync)

    extensions.wrapfilecache(localrepo.localrepository, "dirstate", _wrapdirstate)


def reposetup(ui, repo) -> None:
    if not hasattr(repo, "dirstate"):
        return

    dirstate, cached = localrepo.isfilecached(repo, "dirstate")
    if cached:
        _setupdirstate(repo, dirstate)


def _wrapdirstate(orig, repo):
    dirstate = orig(repo)
    _setupdirstate(repo, dirstate)
    return dirstate


def _setupdirstate(repo, dirstate):
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

    dirstate.addparentchangecallback("dirsync", dirsyncfixup)


def _bypassdirsync(orig, ui, repo, *args, **kwargs):
    _disabled[0] = True
    try:
        return orig(ui, repo, *args, **kwargs)
    finally:
        _disabled[0] = False


def getconfigs(wctx) -> tuple[sortdict, dict]:
    """returns {name: [path]}.
    [path] under a same name are synced. name is not useful.
    """
    # read from .hgdirsync in repo
    filename = ".hgdirsync"
    try:
        content = wctx[filename].data().decode()
    except (error.ManifestLookupError, IOError, AttributeError, KeyError):
        content = ""
    cfg = bindings.configloader.config()
    if content:
        cfg.parse("[dirsync]\n%s" % content, filename)

    maps = util.sortdict()
    scripts = {}
    repo = wctx.repo()
    cfg_items = {name: cfg.get("dirsync", name) for name in cfg.names("dirsync")}
    for key, value in repo.ui.configitems("dirsync") + list(cfg_items.items()):
        if "." not in key:
            continue
        name, disambig = key.split(".", 1)
        # Normalize paths to have / at the end. For easy concatenation later.
        if value[-1] != "/":
            value = value + "/"
        if name not in maps:
            maps[name] = []
        maps[name].append(value)

    scripts.update(repo.ui.configitems("dirsync-scripts"))

    # For security reasons, `[dirsync-scripts]` in `.hgdirsyncrc` is ignored,
    # unless `dirsync.allow-in-repo-scripts` is set to true.
    if repo.ui.configbool("dirsync", "allow-in-repo-scripts"):
        scripts.update(
            {
                name: cfg.get("dirsync-scripts", name)
                for name in cfg.names("dirsync-scripts")
            }
        )

    return maps, scripts


def configstomatcher(configs):
    """returns a matcher matching files need to be dirsynced

    configs is the return value of getconfigs()
    """
    rules = set()
    for mirrors in configs.values():
        for mirror in mirrors:
            assert mirror.endswith("/"), "getconfigs() ensures this"
            rules.add("%s**" % mirror)
    return matchmod.treematcher("", "", rules=sorted(rules))


def getmirrors(maps, filename) -> tuple[Optional[str], Optional[str], list[str]]:
    """Returns (name, srcmirror, mirrors)

    name is the mirror name (config name before ".").
    srcmirror is the mirror path the filename is in.
    mirrors is a list of mirror paths, including srcmirror.

    name and srcmirror can be None.
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
                return None, None, []

    for mirrorname, mirrordirs in maps.items():
        for subdir in mirrordirs:
            if checkpath.startswith(subdir):
                return mirrorname, subdir, mirrordirs

    return None, None, []


def _mctxstatus(ctx, matcher):
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
    mutpredhex = mutinfo and mutinfo["mutpred"].split(",", 1)[0].removeprefix("hg/")
    mutpred = mutpredhex and bin(mutpredhex)
    predctx = mutpred and mutpred in repo and repo[mutpred] or None

    # By default, consider all changes in this commit as needing syncing.
    status = mctx._status

    # But, if there was an amend/absorb predecessor commit, remove any changes
    # that are not different from the predecessor (i.e. things not changed in
    # the amend/absorb) and add any removes that happened since the predecessor.
    if predctx:
        if predctx.p1() == ctx.p1() or ctx.p1().node() in _nodemirrored:
            status = _adjuststatus(status, predctx, ctx, matcher)

    return mctx, status


def load_script(ctx, scripts: dict[str, str], mirrorname: str, cache):
    """load python module (source file defined as scripts[mirrorname])

    Return None if the source file is not in ctx.
    Print warnings if anything goes wrong (like Python syntax error),
    and return None.

    cache is a dict {mirrorname: module} to avoid re-reading files.
    """
    path = scripts.get(mirrorname)
    if not path:
        return None

    result = cache.get(mirrorname)
    if result is not None:
        return result

    ui = ctx.repo().ui
    try:
        script = ctx[path].data().decode("utf-8")
        mod = bindings.hook.load_source(script, f"dirsync_{mirrorname}")
        cache[mirrorname] = mod
        return mod
    except Exception as e:
        ui.warn(
            _(
                "warning: ignored problematic dirsync script %s defined as %s.script in .hgdirsync: %s\n"
            )
            % (path, mirrorname, e)
        )
        return None


def dirsyncctx(ctx, matcher=None):
    """for changes in ctx that matches matcher, apply dirsync rules

    Return:

        (newctx, {path})

    This function does not change working copy or dirstate.
    """
    repo = ctx.repo()
    maps, scripts = getconfigs(ctx)
    resultmirrored = set()
    resultctx = ctx

    # Do not dirsync if there is nothing to sync.
    if not maps:
        repo.ui.note_err(_("dirsync: skipped because dirsync config is empty\n"))
        return resultctx, resultmirrored
    # Do not dirsync metaedit commits, because they might break assertions in
    # metadataonlyctx (manifest is unchanged).
    if (ctx.mutinfo() or {}).get("mutop") == "metaedit":
        repo.ui.note_err(_("dirsync: skipped for metaedit\n"))
        return resultctx, resultmirrored

    needsync = configstomatcher(maps)

    if matcher:
        matcher = matchmod.intersectmatchers(matcher, needsync)
    else:
        matcher = needsync

    # skip dirsync if there is no files match the (needsync) matcher
    if not any(matcher(p) for p in ctx.files()):
        repo.ui.note_err(
            _("dirsync: skipped because files paths do not match dirsync config\n")
        )
        return resultctx, resultmirrored

    mctx, status = _mctxstatus(ctx, matcher)

    added = set(status.added)
    modified = set(status.modified)
    removed = set(status.removed)

    module_cache = {}

    for action, paths in (
        ("a", status.added),
        ("m", status.modified),
        ("r", status.removed),
    ):
        for src in paths:
            mirrorname, srcmirror, mirrors = getmirrors(maps, src)
            if not mirrors:
                repo.ui.debug(
                    _("dirsync: %s file %s has no mirrored path\n") % (action, src)
                )
                continue
            mod = load_script(ctx, scripts, mirrorname, module_cache)
            mirror_path = getattr(mod, "mirror_path", _default_mirror_path)
            mirror_data = getattr(mod, "mirror_data", None)

            dstpaths = []  # [(dstpath, dstmirror)]
            for dstmirror in (m for m in mirrors if m != srcmirror):
                dst = _mirror_full_path(srcmirror, dstmirror, src, mirror_path)
                if dst is not None:
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
                    newcopyfrom = _mirror_full_path(
                        srcmirror, dstmirror, copyfrom, mirror_path
                    )
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

                if mirror_data is not None:
                    src_data = fsrc.data()
                    dst_data = mirror_data(srcmirror, dstmirror, src, src_data)
                    if dst_data != src_data:
                        if not isinstance(dst_data, bytes):
                            raise error.ProgrammingError(
                                "mirror_data result should be bytes, got %s"
                                % type(dst_data)
                            )
                        fmirror = context.overlayfilectx(
                            fmirror, datafunc=lambda data=dst_data: data
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
    else:
        repo.ui.debug(_("dirsync: no files are mirrored\n"))

    return resultctx, resultmirrored


def _default_mirror_path(_srcdir: str, _dstdir: str, relsrc: str):
    """Return `reldst`. Custom scripts can redefine this function.

    The repo-relative path of the source file to mirror is: srcdir + relsrc.
    The repo-relative path of the destination path is: dstdir + return value
    (reldst).
    If return None, the mirror behavior will be cancelled.
    """
    # By default, the relative path in destination dir is the same as the
    # relative path in the source dir.
    return relsrc


def _mirror_full_path(
    srcdir: str, dstdir: str, src: str, mirror_path=_default_mirror_path
):
    """Mirror src path from srcdir to dstdir.
    Return None if src is not in srcdir.

    mirror_path is a function that takes (srcdir, dstdir, relsrc),
    and returns reldst (relative path in the destination directory).
    If mirror_path returns None, then the file won't be mirrored.
    """
    reldst = None
    if src + "/" == srcdir:
        # special case: src is a file to mirror
        reldst = mirror_path(srcdir, dstdir, "")
    elif src.startswith(srcdir):
        relsrc = src[len(srcdir) :]
        reldst = mirror_path(srcdir, dstdir, relsrc)
    if reldst is not None:
        if ".." in reldst and ".." in reldst.split("/"):
            raise error.ProgrammingError(
                "mirror_path result cannot contain '..' path component"
            )
        return (dstdir + reldst).rstrip("/")
    return None


def _adjuststatus(status, ctx1, ctx2, matcher) -> status:
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
            if not matcher(path):
                continue

            # No real changes? (case 1)
            if path in ctx1 and path in ctx2:
                f1 = ctx1[path]
                f2 = ctx2[path]
                if f1.flags() == f2.flags() and not f1.cmp(f2):
                    skipped.add(path)
                    continue
            newpaths.append(path)

    for path in status.removed:
        if not matcher(path):
            continue

        # No real changes (case 1)
        if path not in ctx1 and path not in ctx2:
            skipped.add(path)
            continue
        newremoved.append(path)

    # Add files that have been reverted.
    for oldpath in ctx1.files():
        if not matcher(oldpath):
            continue

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
        ctx.repo().ui.note_err(_("dirsync: skipped for shelve commands\n"))
        return orig(self, ctx, *args, **kwargs)

    ctx, mirrored = dirsyncctx(ctx)
    node = orig(self, ctx, *args, **kwargs)

    if mirrored:
        # used by dirsyncfixup to write back from commit to disk
        _nodemirrored[node] = mirrored

    return node
