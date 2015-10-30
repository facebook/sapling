# dirsync.py - keep two directories synchronized at commit time
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
dirsync is an extension for keeping directories in a repo synchronized.

Configure it by adding the following config options to your .hg/hgrc.

[dirsync]
projectX.dir1 = path/to/dir1
projectX.dir2 = path/dir2

The configs are of the form "group.name = path-to-dir". Every config entry with
the same `group` will be mirrored amongst each other. The `name` is just used to
separate them and is not used anywhere. The `path` is the path to the directory
from the repo root. It must be a directory, but it doesn't matter if you specify
the trailing '/' or not.

Multiple mirror groups can be specified at once, and you can mirror between an
arbitrary number of directories. Ex:

[dirsync]
projectX.dir1 = path/to/dir1
projectX.dir2 = path/dir2
projectY.dir1 = otherpath/dir1
projectY.dir2 = foo/bar
projectY.dir3 = foo/goo/hoo
"""

from collections import defaultdict
from mercurial import extensions, localrepo, util

testedwith = 'internal'

def extsetup(ui):
    extensions.wrapfunction(localrepo.localrepository, 'commit', _commit)

def getconfigs(ui):
    maps = defaultdict(list)
    for key, value in ui.configitems('dirsync'):
        if '.' not in key:
            continue
        name, disambig = key.split('.', 1)
        # Normalize paths to have / at the end. For easy concatenation later.
        if value[-1] != '/':
            value = value + '/'
        maps[name].append(value)

    return maps

def getmirrors(maps, filename):
    for key, mirrordirs in maps.iteritems():
        for subdir in mirrordirs:
            if filename.startswith(subdir):
                return mirrordirs

    return []

def _commit(orig, self, *args, **kwargs):
    wlock = self.wlock()
    try:
        maps = getconfigs(self.ui)
        if maps:
            match = kwargs.get('match', None)
            status = self.status(match=match)

            for added in status.added:
                mirrors = getmirrors(maps, added)
                if mirrors:
                    applytomirrors(self, status, added, mirrors, 'a')

            for modified in status.modified:
                mirrors = getmirrors(maps, modified)
                if mirrors:
                    applytomirrors(self, status, modified, mirrors, 'm')

            for removed in status.removed:
                mirrors = getmirrors(maps, removed)
                if mirrors:
                    applytomirrors(self, status, removed, mirrors, 'r')

        return orig(self, *args, **kwargs)
    finally:
        wlock.release()

def applytomirrors(repo, status, sourcepath, mirrors, action):
    """Applies the changes that are in the sourcepath to all the mirrors."""
    # Detect which mirror this file comes from
    sourcemirror = None
    for mirror in mirrors:
        if sourcepath.startswith(mirror):
            sourcemirror = mirror
            break
    if not sourcemirror:
        raise Exception("unable to detect source mirror of '%s'" % sourcepath)

    relpath = sourcepath[len(sourcemirror):]

    # Apply the change to each mirror one by one
    allchanges = set(status.modified + status.removed + status.added)
    for mirror in mirrors:
        if mirror == sourcemirror:
            continue

        mirrorpath = mirror + relpath
        if mirrorpath in allchanges:
            wctx = repo[None]
            if (sourcepath not in wctx and mirrorpath not in wctx and
                sourcepath in status.removed and mirrorpath in status.removed):
                repo.ui.status("not mirroring remove of '%s' to '%s'; it is "
                               "already removed\n" % (sourcepath, mirrorpath))
                continue

            if wctx[sourcepath].data() == wctx[mirrorpath].data():
                repo.ui.status("not mirroring '%s' to '%s'; it already "
                               "matches\n" % (sourcepath, mirrorpath))
                continue
            raise util.Abort("path '%s' needs to be mirrored to '%s', but the "
                             "target already has pending changes" %
                             (sourcepath, mirrorpath))

        fullsource = repo.wjoin(sourcepath)
        fulltarget = repo.wjoin(mirrorpath)

        dirstate = repo.dirstate
        if action == 'm' or action == 'a':
            mirrorpathdir, unused = util.split(mirrorpath)
            util.makedirs(repo.wjoin(mirrorpathdir))

            util.copyfile(fullsource, fulltarget)

            if action == 'a':
                dirstate.add(mirrorpath)

                # For adds, detect copy data as well
                copysource = dirstate.copied(sourcepath)
                if copysource and copysource.startswith(sourcemirror):
                    mirrorcopysource = mirror + copysource[len(sourcemirror):]
                    dirstate.copy(mirrorcopysource, mirrorpath)
                    repo.ui.status("mirrored copy '%s -> %s' to '%s -> %s'\n" %
                                   (copysource, sourcepath,
                                    mirrorcopysource, mirrorpath))
                else:
                    repo.ui.status("mirrored adding '%s' to '%s'\n" %
                                   (sourcepath, mirrorpath))
            else:
                repo.ui.status("mirrored changes in '%s' to '%s'\n" %
                               (sourcepath, mirrorpath))
        elif action == 'r':
            util.unlink(fulltarget)
            dirstate.remove(mirrorpath)
            repo.ui.status("mirrored remove of '%s' to '%s'\n" % (sourcepath, mirrorpath))
