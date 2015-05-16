# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''largefiles utility code: must not import other modules in this package.'''

import os
import platform
import shutil
import stat
import copy

from mercurial import dirstate, httpconnection, match as match_, util, scmutil
from mercurial.i18n import _
from mercurial import node

shortname = '.hglf'
shortnameslash = shortname + '/'
longname = 'largefiles'


# -- Private worker functions ------------------------------------------

def getminsize(ui, assumelfiles, opt, default=10):
    lfsize = opt
    if not lfsize and assumelfiles:
        lfsize = ui.config(longname, 'minsize', default=default)
    if lfsize:
        try:
            lfsize = float(lfsize)
        except ValueError:
            raise util.Abort(_('largefiles: size must be number (not %s)\n')
                             % lfsize)
    if lfsize is None:
        raise util.Abort(_('minimum size for largefiles must be specified'))
    return lfsize

def link(src, dest):
    util.makedirs(os.path.dirname(dest))
    try:
        util.oslink(src, dest)
    except OSError:
        # if hardlinks fail, fallback on atomic copy
        dst = util.atomictempfile(dest)
        for chunk in util.filechunkiter(open(src, 'rb')):
            dst.write(chunk)
        dst.close()
        os.chmod(dest, os.stat(src).st_mode)

def usercachepath(ui, hash):
    path = ui.configpath(longname, 'usercache', None)
    if path:
        path = os.path.join(path, hash)
    else:
        if os.name == 'nt':
            appdata = os.getenv('LOCALAPPDATA', os.getenv('APPDATA'))
            if appdata:
                path = os.path.join(appdata, longname, hash)
        elif platform.system() == 'Darwin':
            home = os.getenv('HOME')
            if home:
                path = os.path.join(home, 'Library', 'Caches',
                                    longname, hash)
        elif os.name == 'posix':
            path = os.getenv('XDG_CACHE_HOME')
            if path:
                path = os.path.join(path, longname, hash)
            else:
                home = os.getenv('HOME')
                if home:
                    path = os.path.join(home, '.cache', longname, hash)
        else:
            raise util.Abort(_('unknown operating system: %s\n') % os.name)
    return path

def inusercache(ui, hash):
    path = usercachepath(ui, hash)
    return path and os.path.exists(path)

def findfile(repo, hash):
    path, exists = findstorepath(repo, hash)
    if exists:
        repo.ui.note(_('found %s in store\n') % hash)
        return path
    elif inusercache(repo.ui, hash):
        repo.ui.note(_('found %s in system cache\n') % hash)
        path = storepath(repo, hash)
        link(usercachepath(repo.ui, hash), path)
        return path
    return None

class largefilesdirstate(dirstate.dirstate):
    def __getitem__(self, key):
        return super(largefilesdirstate, self).__getitem__(unixpath(key))
    def normal(self, f):
        return super(largefilesdirstate, self).normal(unixpath(f))
    def remove(self, f):
        return super(largefilesdirstate, self).remove(unixpath(f))
    def add(self, f):
        return super(largefilesdirstate, self).add(unixpath(f))
    def drop(self, f):
        return super(largefilesdirstate, self).drop(unixpath(f))
    def forget(self, f):
        return super(largefilesdirstate, self).forget(unixpath(f))
    def normallookup(self, f):
        return super(largefilesdirstate, self).normallookup(unixpath(f))
    def _ignore(self, f):
        return False

def openlfdirstate(ui, repo, create=True):
    '''
    Return a dirstate object that tracks largefiles: i.e. its root is
    the repo root, but it is saved in .hg/largefiles/dirstate.
    '''
    lfstoredir = repo.join(longname)
    opener = scmutil.opener(lfstoredir)
    lfdirstate = largefilesdirstate(opener, ui, repo.root,
                                     repo.dirstate._validate)

    # If the largefiles dirstate does not exist, populate and create
    # it. This ensures that we create it on the first meaningful
    # largefiles operation in a new clone.
    if create and not os.path.exists(os.path.join(lfstoredir, 'dirstate')):
        matcher = getstandinmatcher(repo)
        standins = repo.dirstate.walk(matcher, [], False, False)

        if len(standins) > 0:
            util.makedirs(lfstoredir)

        for standin in standins:
            lfile = splitstandin(standin)
            lfdirstate.normallookup(lfile)
    return lfdirstate

def lfdirstatestatus(lfdirstate, repo):
    wctx = repo['.']
    match = match_.always(repo.root, repo.getcwd())
    unsure, s = lfdirstate.status(match, [], False, False, False)
    modified, clean = s.modified, s.clean
    for lfile in unsure:
        try:
            fctx = wctx[standin(lfile)]
        except LookupError:
            fctx = None
        if not fctx or fctx.data().strip() != hashfile(repo.wjoin(lfile)):
            modified.append(lfile)
        else:
            clean.append(lfile)
            lfdirstate.normal(lfile)
    return s

def listlfiles(repo, rev=None, matcher=None):
    '''return a list of largefiles in the working copy or the
    specified changeset'''

    if matcher is None:
        matcher = getstandinmatcher(repo)

    # ignore unknown files in working directory
    return [splitstandin(f)
            for f in repo[rev].walk(matcher)
            if rev is not None or repo.dirstate[f] != '?']

def instore(repo, hash, forcelocal=False):
    return os.path.exists(storepath(repo, hash, forcelocal))

def storepath(repo, hash, forcelocal=False):
    if not forcelocal and repo.shared():
        return repo.vfs.reljoin(repo.sharedpath, longname, hash)
    return repo.join(longname, hash)

def findstorepath(repo, hash):
    '''Search through the local store path(s) to find the file for the given
    hash.  If the file is not found, its path in the primary store is returned.
    The return value is a tuple of (path, exists(path)).
    '''
    # For shared repos, the primary store is in the share source.  But for
    # backward compatibility, force a lookup in the local store if it wasn't
    # found in the share source.
    path = storepath(repo, hash, False)

    if instore(repo, hash):
        return (path, True)
    elif repo.shared() and instore(repo, hash, True):
        return storepath(repo, hash, True)

    return (path, False)

def copyfromcache(repo, hash, filename):
    '''Copy the specified largefile from the repo or system cache to
    filename in the repository. Return true on success or false if the
    file was not found in either cache (which should not happened:
    this is meant to be called only after ensuring that the needed
    largefile exists in the cache).'''
    path = findfile(repo, hash)
    if path is None:
        return False
    util.makedirs(os.path.dirname(repo.wjoin(filename)))
    # The write may fail before the file is fully written, but we
    # don't use atomic writes in the working copy.
    shutil.copy(path, repo.wjoin(filename))
    return True

def copytostore(repo, rev, file, uploaded=False):
    hash = readstandin(repo, file, rev)
    if instore(repo, hash):
        return
    copytostoreabsolute(repo, repo.wjoin(file), hash)

def copyalltostore(repo, node):
    '''Copy all largefiles in a given revision to the store'''

    ctx = repo[node]
    for filename in ctx.files():
        if isstandin(filename) and filename in ctx.manifest():
            realfile = splitstandin(filename)
            copytostore(repo, ctx.node(), realfile)


def copytostoreabsolute(repo, file, hash):
    if inusercache(repo.ui, hash):
        link(usercachepath(repo.ui, hash), storepath(repo, hash))
    else:
        util.makedirs(os.path.dirname(storepath(repo, hash)))
        dst = util.atomictempfile(storepath(repo, hash),
                                  createmode=repo.store.createmode)
        for chunk in util.filechunkiter(open(file, 'rb')):
            dst.write(chunk)
        dst.close()
        linktousercache(repo, hash)

def linktousercache(repo, hash):
    path = usercachepath(repo.ui, hash)
    if path:
        link(storepath(repo, hash), path)

def getstandinmatcher(repo, pats=[], opts={}):
    '''Return a match object that applies pats to the standin directory'''
    standindir = repo.wjoin(shortname)
    if pats:
        pats = [os.path.join(standindir, pat) for pat in pats]
    else:
        # no patterns: relative to repo root
        pats = [standindir]
    # no warnings about missing files or directories
    match = scmutil.match(repo[None], pats, opts)
    match.bad = lambda f, msg: None
    return match

def composestandinmatcher(repo, rmatcher):
    '''Return a matcher that accepts standins corresponding to the
    files accepted by rmatcher. Pass the list of files in the matcher
    as the paths specified by the user.'''
    smatcher = getstandinmatcher(repo, rmatcher.files())
    isstandin = smatcher.matchfn
    def composedmatchfn(f):
        return isstandin(f) and rmatcher.matchfn(splitstandin(f))
    smatcher.matchfn = composedmatchfn

    return smatcher

def standin(filename):
    '''Return the repo-relative path to the standin for the specified big
    file.'''
    # Notes:
    # 1) Some callers want an absolute path, but for instance addlargefiles
    #    needs it repo-relative so it can be passed to repo[None].add().  So
    #    leave it up to the caller to use repo.wjoin() to get an absolute path.
    # 2) Join with '/' because that's what dirstate always uses, even on
    #    Windows. Change existing separator to '/' first in case we are
    #    passed filenames from an external source (like the command line).
    return shortnameslash + util.pconvert(filename)

def isstandin(filename):
    '''Return true if filename is a big file standin. filename must be
    in Mercurial's internal form (slash-separated).'''
    return filename.startswith(shortnameslash)

def splitstandin(filename):
    # Split on / because that's what dirstate always uses, even on Windows.
    # Change local separator to / first just in case we are passed filenames
    # from an external source (like the command line).
    bits = util.pconvert(filename).split('/', 1)
    if len(bits) == 2 and bits[0] == shortname:
        return bits[1]
    else:
        return None

def updatestandin(repo, standin):
    file = repo.wjoin(splitstandin(standin))
    if os.path.exists(file):
        hash = hashfile(file)
        executable = getexecutable(file)
        writestandin(repo, standin, hash, executable)

def readstandin(repo, filename, node=None):
    '''read hex hash from standin for filename at given node, or working
    directory if no node is given'''
    return repo[node][standin(filename)].data().strip()

def writestandin(repo, standin, hash, executable):
    '''write hash to <repo.root>/<standin>'''
    repo.wwrite(standin, hash + '\n', executable and 'x' or '')

def copyandhash(instream, outfile):
    '''Read bytes from instream (iterable) and write them to outfile,
    computing the SHA-1 hash of the data along the way. Return the hash.'''
    hasher = util.sha1('')
    for data in instream:
        hasher.update(data)
        outfile.write(data)
    return hasher.hexdigest()

def hashrepofile(repo, file):
    return hashfile(repo.wjoin(file))

def hashfile(file):
    if not os.path.exists(file):
        return ''
    hasher = util.sha1('')
    fd = open(file, 'rb')
    for data in util.filechunkiter(fd, 128 * 1024):
        hasher.update(data)
    fd.close()
    return hasher.hexdigest()

def getexecutable(filename):
    mode = os.stat(filename).st_mode
    return ((mode & stat.S_IXUSR) and
            (mode & stat.S_IXGRP) and
            (mode & stat.S_IXOTH))

def urljoin(first, second, *arg):
    def join(left, right):
        if not left.endswith('/'):
            left += '/'
        if right.startswith('/'):
            right = right[1:]
        return left + right

    url = join(first, second)
    for a in arg:
        url = join(url, a)
    return url

def hexsha1(data):
    """hexsha1 returns the hex-encoded sha1 sum of the data in the file-like
    object data"""
    h = util.sha1()
    for chunk in util.filechunkiter(data):
        h.update(chunk)
    return h.hexdigest()

def httpsendfile(ui, filename):
    return httpconnection.httpsendfile(ui, filename, 'rb')

def unixpath(path):
    '''Return a version of path normalized for use with the lfdirstate.'''
    return util.pconvert(os.path.normpath(path))

def islfilesrepo(repo):
    if ('largefiles' in repo.requirements and
            any(shortnameslash in f[0] for f in repo.store.datafiles())):
        return True

    return any(openlfdirstate(repo.ui, repo, False))

class storeprotonotcapable(Exception):
    def __init__(self, storetypes):
        self.storetypes = storetypes

def getstandinsstate(repo):
    standins = []
    matcher = getstandinmatcher(repo)
    for standin in repo.dirstate.walk(matcher, [], False, False):
        lfile = splitstandin(standin)
        try:
            hash = readstandin(repo, lfile)
        except IOError:
            hash = None
        standins.append((lfile, hash))
    return standins

def synclfdirstate(repo, lfdirstate, lfile, normallookup):
    lfstandin = standin(lfile)
    if lfstandin in repo.dirstate:
        stat = repo.dirstate._map[lfstandin]
        state, mtime = stat[0], stat[3]
    else:
        state, mtime = '?', -1
    if state == 'n':
        if normallookup or mtime < 0:
            # state 'n' doesn't ensure 'clean' in this case
            lfdirstate.normallookup(lfile)
        else:
            lfdirstate.normal(lfile)
    elif state == 'm':
        lfdirstate.normallookup(lfile)
    elif state == 'r':
        lfdirstate.remove(lfile)
    elif state == 'a':
        lfdirstate.add(lfile)
    elif state == '?':
        lfdirstate.drop(lfile)

def markcommitted(orig, ctx, node):
    repo = ctx.repo()

    orig(node)

    # ATTENTION: "ctx.files()" may differ from "repo[node].files()"
    # because files coming from the 2nd parent are omitted in the latter.
    #
    # The former should be used to get targets of "synclfdirstate",
    # because such files:
    # - are marked as "a" by "patch.patch()" (e.g. via transplant), and
    # - have to be marked as "n" after commit, but
    # - aren't listed in "repo[node].files()"

    lfdirstate = openlfdirstate(repo.ui, repo)
    for f in ctx.files():
        if isstandin(f):
            lfile = splitstandin(f)
            synclfdirstate(repo, lfdirstate, lfile, False)
    lfdirstate.write()

    # As part of committing, copy all of the largefiles into the cache.
    copyalltostore(repo, node)

def getlfilestoupdate(oldstandins, newstandins):
    changedstandins = set(oldstandins).symmetric_difference(set(newstandins))
    filelist = []
    for f in changedstandins:
        if f[0] not in filelist:
            filelist.append(f[0])
    return filelist

def getlfilestoupload(repo, missing, addfunc):
    for i, n in enumerate(missing):
        repo.ui.progress(_('finding outgoing largefiles'), i,
            unit=_('revision'), total=len(missing))
        parents = [p for p in repo.changelog.parents(n) if p != node.nullid]

        oldlfstatus = repo.lfstatus
        repo.lfstatus = False
        try:
            ctx = repo[n]
        finally:
            repo.lfstatus = oldlfstatus

        files = set(ctx.files())
        if len(parents) == 2:
            mc = ctx.manifest()
            mp1 = ctx.parents()[0].manifest()
            mp2 = ctx.parents()[1].manifest()
            for f in mp1:
                if f not in mc:
                    files.add(f)
            for f in mp2:
                if f not in mc:
                    files.add(f)
            for f in mc:
                if mc[f] != mp1.get(f, None) or mc[f] != mp2.get(f, None):
                    files.add(f)
        for fn in files:
            if isstandin(fn) and fn in ctx:
                addfunc(fn, ctx[fn].data().strip())
    repo.ui.progress(_('finding outgoing largefiles'), None)

def updatestandinsbymatch(repo, match):
    '''Update standins in the working directory according to specified match

    This returns (possibly modified) ``match`` object to be used for
    subsequent commit process.
    '''

    ui = repo.ui

    # Case 1: user calls commit with no specific files or
    # include/exclude patterns: refresh and commit all files that
    # are "dirty".
    if match is None or match.always():
        # Spend a bit of time here to get a list of files we know
        # are modified so we can compare only against those.
        # It can cost a lot of time (several seconds)
        # otherwise to update all standins if the largefiles are
        # large.
        lfdirstate = openlfdirstate(ui, repo)
        dirtymatch = match_.always(repo.root, repo.getcwd())
        unsure, s = lfdirstate.status(dirtymatch, [], False, False,
                                      False)
        modifiedfiles = unsure + s.modified + s.added + s.removed
        lfiles = listlfiles(repo)
        # this only loops through largefiles that exist (not
        # removed/renamed)
        for lfile in lfiles:
            if lfile in modifiedfiles:
                if os.path.exists(
                        repo.wjoin(standin(lfile))):
                    # this handles the case where a rebase is being
                    # performed and the working copy is not updated
                    # yet.
                    if os.path.exists(repo.wjoin(lfile)):
                        updatestandin(repo,
                            standin(lfile))

        return match

    lfiles = listlfiles(repo)
    match._files = repo._subdirlfs(match.files(), lfiles)

    # Case 2: user calls commit with specified patterns: refresh
    # any matching big files.
    smatcher = composestandinmatcher(repo, match)
    standins = repo.dirstate.walk(smatcher, [], False, False)

    # No matching big files: get out of the way and pass control to
    # the usual commit() method.
    if not standins:
        return match

    # Refresh all matching big files.  It's possible that the
    # commit will end up failing, in which case the big files will
    # stay refreshed.  No harm done: the user modified them and
    # asked to commit them, so sooner or later we're going to
    # refresh the standins.  Might as well leave them refreshed.
    lfdirstate = openlfdirstate(ui, repo)
    for fstandin in standins:
        lfile = splitstandin(fstandin)
        if lfdirstate[lfile] != 'r':
            updatestandin(repo, fstandin)

    # Cook up a new matcher that only matches regular files or
    # standins corresponding to the big files requested by the
    # user.  Have to modify _files to prevent commit() from
    # complaining "not tracked" for big files.
    match = copy.copy(match)
    origmatchfn = match.matchfn

    # Check both the list of largefiles and the list of
    # standins because if a largefile was removed, it
    # won't be in the list of largefiles at this point
    match._files += sorted(standins)

    actualfiles = []
    for f in match._files:
        fstandin = standin(f)

        # ignore known largefiles and standins
        if f in lfiles or fstandin in standins:
            continue

        actualfiles.append(f)
    match._files = actualfiles

    def matchfn(f):
        if origmatchfn(f):
            return f not in lfiles
        else:
            return f in standins

    match.matchfn = matchfn

    return match

class automatedcommithook(object):
    '''Stateful hook to update standins at the 1st commit of resuming

    For efficiency, updating standins in the working directory should
    be avoided while automated committing (like rebase, transplant and
    so on), because they should be updated before committing.

    But the 1st commit of resuming automated committing (e.g. ``rebase
    --continue``) should update them, because largefiles may be
    modified manually.
    '''
    def __init__(self, resuming):
        self.resuming = resuming

    def __call__(self, repo, match):
        if self.resuming:
            self.resuming = False # avoids updating at subsequent commits
            return updatestandinsbymatch(repo, match)
        else:
            return match

def getstatuswriter(ui, repo, forcibly=None):
    '''Return the function to write largefiles specific status out

    If ``forcibly`` is ``None``, this returns the last element of
    ``repo._lfstatuswriters`` as "default" writer function.

    Otherwise, this returns the function to always write out (or
    ignore if ``not forcibly``) status.
    '''
    if forcibly is None and util.safehasattr(repo, '_largefilesenabled'):
        return repo._lfstatuswriters[-1]
    else:
        if forcibly:
            return ui.status # forcibly WRITE OUT
        else:
            return lambda *msg, **opts: None # forcibly IGNORE
