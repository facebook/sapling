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
import errno
from mercurial import commands, extensions, localrepo, util
from mercurial import match as matchmod
from mercurial import error
from mercurial.i18n import _

testedwith = 'internal'

def extsetup(ui):
    extensions.wrapfunction(localrepo.localrepository, 'commit', _commit)
    def wrapshelve(loaded=False):
        try:
            shelvemod = extensions.find('shelve')
            extensions.wrapcommand(shelvemod.cmdtable, 'shelve',
                                   _bypassdirsync)
            extensions.wrapcommand(shelvemod.cmdtable, 'unshelve',
                                   _bypassdirsync)
        except KeyError:
            pass
    extensions.afterloaded('shelve', wrapshelve)

def _bypassdirsync(orig, ui, repo, *args, **kwargs):
    backup = ui.backupconfig('dirsync', '_tempdisable')
    try:
        ui.setconfig('dirsync', '_tempdisable', True)
        return orig(ui, repo, *args, **kwargs)
    finally:
        ui.restoreconfig(backup)

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
    if self.ui.configbool('dirsync', '_tempdisable', False):
        return orig(self, *args, **kwargs)

    wlock = self.wlock()
    try:
        maps = getconfigs(self.ui)
        mirroredfiles = set()
        if maps:
            match = args[3] if len(args) >= 4 else kwargs.get('match')
            match = match or matchmod.always(self.root, '')
            status = self.status()

            for added in status.added:
                mirrors = getmirrors(maps, added)
                if mirrors and match(added):
                    mirroredfiles.update(applytomirrors(self, status, added,
                        mirrors, 'a'))

            for modified in status.modified:
                mirrors = getmirrors(maps, modified)
                if mirrors and match(modified):
                    mirroredfiles.update(applytomirrors(self, status, modified,
                        mirrors, 'm'))

            for removed in status.removed:
                mirrors = getmirrors(maps, removed)
                if mirrors and match(removed):
                    mirroredfiles.update(applytomirrors(self, status, removed,
                        mirrors, 'r'))

        if mirroredfiles and not match.always():
            origmatch = match.matchfn
            def extramatches(path):
                return path in mirroredfiles or origmatch(path)
            match.matchfn = extramatches
            match._files.extend(mirroredfiles)
            match._fileroots.update(mirroredfiles)
        return orig(self, *args, **kwargs)
    finally:
        wlock.release()

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
        raise error.Abort(_("unable to detect source mirror of '%s'") %
                          (sourcepath,))

    relpath = sourcepath[len(sourcemirror):]

    # Apply the change to each mirror one by one
    allchanges = set(status.modified + status.removed + status.added)
    for mirror in mirrors:
        if mirror == sourcemirror:
            continue

        mirrorpath = mirror + relpath
        mirroredfiles.add(mirrorpath)
        if mirrorpath in allchanges:
            wctx = repo[None]
            if (sourcepath not in wctx and mirrorpath not in wctx and
                sourcepath in status.removed and mirrorpath in status.removed):
                if repo.ui.verbose:
                    repo.ui.status(_("not mirroring remove of '%s' to '%s';"
                                     " it is already removed\n")
                                   % (sourcepath, mirrorpath))
                continue

            if wctx[sourcepath].data() == wctx[mirrorpath].data():
                if repo.ui.verbose:
                    repo.ui.status(_("not mirroring '%s' to '%s'; it already "
                                     "matches\n") % (sourcepath, mirrorpath))
                continue
            raise error.Abort(_("path '%s' needs to be mirrored to '%s', but "
                                "the target already has pending changes") %
                              (sourcepath, mirrorpath))

        fullsource = repo.wjoin(sourcepath)
        fulltarget = repo.wjoin(mirrorpath)

        dirstate = repo.dirstate
        if action == 'm' or action == 'a':
            mirrorpathdir, unused = util.split(mirrorpath)
            util.makedirs(repo.wjoin(mirrorpathdir))

            util.copyfile(fullsource, fulltarget)
            if dirstate[mirrorpath] in '?r':
                dirstate.add(mirrorpath)


            if action == 'a':
                # For adds, detect copy data as well
                copysource = dirstate.copied(sourcepath)
                if copysource and copysource.startswith(sourcemirror):
                    mirrorcopysource = mirror + copysource[len(sourcemirror):]
                    dirstate.copy(mirrorcopysource, mirrorpath)
                    repo.ui.status(_("mirrored copy '%s -> %s' to '%s -> %s'\n")
                                   % (copysource, sourcepath,
                                      mirrorcopysource, mirrorpath))
                else:
                    repo.ui.status(_("mirrored adding '%s' to '%s'\n") %
                                   (sourcepath, mirrorpath))
            else:
                repo.ui.status(_("mirrored changes in '%s' to '%s'\n") %
                               (sourcepath, mirrorpath))
        elif action == 'r':
            try:
                util.unlink(fulltarget)
            except OSError as e:
                if e.errno == errno.ENOENT:
                    repo.ui.status(_("not mirroring remove of '%s' to '%s'; it "
                                     "is already removed\n") %
                                   (sourcepath, mirrorpath))
                else:
                    raise
            else:
                dirstate.remove(mirrorpath)
                repo.ui.status(_("mirrored remove of '%s' to '%s'\n") %
                               (sourcepath, mirrorpath))

    return mirroredfiles
