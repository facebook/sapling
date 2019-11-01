# Copyright (c) Facebook, Inc. and its affiliates.
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

import errno

from edenscm.mercurial import (
    cmdutil,
    config,
    error,
    extensions,
    localrepo,
    match as matchmod,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _


testedwith = "ships-with-fb-hgext"
EXCLUDE_PATHS = "exclude"

_disabled = [False]


def extsetup(ui):
    extensions.wrapfunction(cmdutil, "amend", _amend)
    extensions.wrapfunction(localrepo.localrepository, "commit", _commit)

    def wrapshelve(loaded=False):
        try:
            shelvemod = extensions.find("shelve")
            extensions.wrapcommand(shelvemod.cmdtable, "shelve", _bypassdirsync)
            extensions.wrapcommand(shelvemod.cmdtable, "unshelve", _bypassdirsync)
        except KeyError:
            pass

    extensions.afterloaded("shelve", wrapshelve)


def _bypassdirsync(orig, ui, repo, *args, **kwargs):
    _disabled[0] = True
    try:
        return orig(ui, repo, *args, **kwargs)
    finally:
        _disabled[0] = False


def getconfigs(repo):
    # read from wvfs/.hgdirsync
    filename = ".hgdirsync"
    content = repo.wvfs.tryread(filename)
    cfg = config.config()
    if content:
        cfg.parse(filename, "[dirsync]\n%s" % content, ["dirsync"])

    maps = util.sortdict()
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


def getmirrors(maps, filename):
    if EXCLUDE_PATHS in maps:
        for subdir in maps[EXCLUDE_PATHS]:
            if filename.startswith(subdir):
                return []

    for key, mirrordirs in maps.iteritems():
        for subdir in mirrordirs:
            if filename.startswith(subdir):
                return mirrordirs

    return []


def _updateworkingcopy(repo, matcher):
    maps = getconfigs(repo)
    mirroredfiles = set()
    if maps:
        status = repo.status()

        for added in status.added:
            mirrors = getmirrors(maps, added)
            if mirrors and matcher(added):
                mirroredfiles.update(applytomirrors(repo, status, added, mirrors, "a"))

        for modified in status.modified:
            mirrors = getmirrors(maps, modified)
            if mirrors and matcher(modified):
                mirroredfiles.update(
                    applytomirrors(repo, status, modified, mirrors, "m")
                )

        for removed in status.removed:
            mirrors = getmirrors(maps, removed)
            if mirrors and matcher(removed):
                mirroredfiles.update(
                    applytomirrors(repo, status, removed, mirrors, "r")
                )

    return mirroredfiles


def _amend(orig, ui, repo, old, extra, pats, opts):
    # Only wrap if not disabled and repo is instance of
    # localrepo.localrepository
    if _disabled[0] or not isinstance(repo, localrepo.localrepository):
        return orig(ui, repo, old, extra, pats, opts)

    with repo.wlock(), repo.lock(), repo.transaction("dirsyncamend"):
        wctx = repo[None]
        matcher = scmutil.match(wctx, pats, opts)
        if opts.get("addremove") and scmutil.addremove(repo, matcher, "", opts):
            raise error.Abort(
                _("failed to mark all new/missing files as added/removed")
            )

        mirroredfiles = _updateworkingcopy(repo, matcher)
        if mirroredfiles and not matcher.always():
            # Ensure that all the files to be amended (original + synced) are
            # under consideration during the amend operation. We do so by
            # setting the value against 'include' key in opts as the only source
            # of truth.
            pats = ()
            opts["include"] = [f for f in wctx.files() if matcher(f)] + list(
                mirroredfiles
            )

        return orig(ui, repo, old, extra, pats, opts)


def _commit(orig, self, *args, **kwargs):
    if _disabled[0]:
        return orig(self, *args, **kwargs)

    with self.wlock(), self.lock(), self.transaction("dirsynccommit"):
        matcher = args[3] if len(args) >= 4 else kwargs.get("match")
        matcher = matcher or matchmod.always(self.root, "")

        mirroredfiles = _updateworkingcopy(self, matcher)
        if mirroredfiles and not matcher.always():
            origmatch = matcher.matchfn

            def extramatches(path):
                return path in mirroredfiles or origmatch(path)

            matcher.matchfn = extramatches
            matcher._files.extend(mirroredfiles)
            matcher._fileset.update(mirroredfiles)

        return orig(self, *args, **kwargs)


def applytomirrors(repo, status, sourcepath, mirrors, action):
    """Applies the changes that are in the sourcepath to all the mirrors."""
    mirroredfiles = set()

    # Detect which mirror this file comes from
    sourcemirror = None
    for mirror in mirrors:
        if sourcepath.startswith(mirror):
            sourcemirror = mirror
            break
    if not sourcemirror:
        raise error.Abort(_("unable to detect source mirror of '%s'") % (sourcepath,))

    relpath = sourcepath[len(sourcemirror) :]

    # Apply the change to each mirror one by one
    allchanges = set(status.modified + status.removed + status.added)
    for mirror in mirrors:
        if mirror == sourcemirror:
            continue

        mirrorpath = mirror + relpath
        mirroredfiles.add(mirrorpath)
        if mirrorpath in allchanges:
            wctx = repo[None]
            if (
                sourcepath not in wctx
                and mirrorpath not in wctx
                and sourcepath in status.removed
                and mirrorpath in status.removed
            ):
                if repo.ui.verbose:
                    repo.ui.status(
                        _(
                            "not mirroring remove of '%s' to '%s';"
                            " it is already removed\n"
                        )
                        % (sourcepath, mirrorpath)
                    )
                continue

            if wctx[sourcepath].data() == wctx[mirrorpath].data():
                if repo.ui.verbose:
                    repo.ui.status(
                        _("not mirroring '%s' to '%s'; it already " "matches\n")
                        % (sourcepath, mirrorpath)
                    )
                continue
            raise error.Abort(
                _(
                    "path '%s' needs to be mirrored to '%s', but "
                    "the target already has pending changes"
                )
                % (sourcepath, mirrorpath)
            )

        fullsource = repo.wjoin(sourcepath)
        fulltarget = repo.wjoin(mirrorpath)

        dirstate = repo.dirstate
        if action == "m" or action == "a":
            mirrorpathdir, unused = util.split(mirrorpath)
            util.makedirs(repo.wjoin(mirrorpathdir))

            util.copyfile(fullsource, fulltarget)
            if dirstate[mirrorpath] in "?r":
                dirstate.add(mirrorpath)

            if action == "a":
                # For adds, detect copy data as well
                copysource = dirstate.copied(sourcepath)
                if copysource and copysource.startswith(sourcemirror):
                    mirrorcopysource = mirror + copysource[len(sourcemirror) :]
                    dirstate.copy(mirrorcopysource, mirrorpath)
                    repo.ui.status(
                        _("mirrored copy '%s -> %s' to '%s -> %s'\n")
                        % (copysource, sourcepath, mirrorcopysource, mirrorpath)
                    )
                else:
                    repo.ui.status(
                        _("mirrored adding '%s' to '%s'\n") % (sourcepath, mirrorpath)
                    )
            else:
                repo.ui.status(
                    _("mirrored changes in '%s' to '%s'\n") % (sourcepath, mirrorpath)
                )
        elif action == "r":
            try:
                util.unlink(fulltarget)
            except OSError as e:
                if e.errno == errno.ENOENT:
                    repo.ui.status(
                        _(
                            "not mirroring remove of '%s' to '%s'; it "
                            "is already removed\n"
                        )
                        % (sourcepath, mirrorpath)
                    )
                else:
                    raise
            else:
                dirstate.remove(mirrorpath)
                repo.ui.status(
                    _("mirrored remove of '%s' to '%s'\n") % (sourcepath, mirrorpath)
                )

    return mirroredfiles
