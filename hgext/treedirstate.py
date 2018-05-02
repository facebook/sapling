# Copyright Facebook, Inc. 2017
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""tree-based dirstate implementation

::

    [treedirstate]
    # Whether treedirstate is currently being used.
    enabled = False

    # Whether new repos should have treedirstate enabled.
    useinnewrepos = False

    # Whether to upgrade repos to treedirstate on pull.
    upgradeonpull = False

    # Whether to downgrade repos away from treedirstate on pull.
    downgradeonpull = False

    # Minimum size before a tree file will be repacked.
    minrepackthreshold = 1048576

    # Number of times a tree file can grow by before it is repacked.
    repackfactor = 3

    # Percentage probability of performing a cleanup after a write to a
    # treedirstate file that doesn't involve a repack.
    cleanuppercent = 1

    # Verify trees on each update by re-reading the tree root.
    verify = True
"""

from __future__ import absolute_import

import binascii
import errno
import heapq
import itertools
import os
import random
import string
import struct
import time

from mercurial.i18n import _
from mercurial import (
    commands,
    dirstate,
    encoding,
    error,
    extensions,
    localrepo,
    node,
    pycompat,
    registrar,
    scmutil,
    txnutil,
    util,
)

from .extlib import treedirstate as rusttreedirstate

dirstateheader = b'########################treedirstate####'
treedirstateversion = 1
treefileprefix = 'dirstate.tree.'

configtable = {}
configitem = registrar.configitem(configtable)
configitem('treedirstate', 'useinnewrepos', default=True)
configitem('treedirstate', 'upgradeonpull', default=False)
configitem('treedirstate', 'downgradeonpull', default=False)
configitem('treedirstate', 'cleanuppercent', default=1)

# Sentinel length value for when a nonnormalset or otherparentset is absent.
setabsent = 0xffffffff

# Minimum size the treedirstate file can be before auto-repacking.
configitem('treedirstate', 'minrepackthreshold', default=1024 * 1024)

# Number of times the treedirstate file can grow by, compared to its initial
# size, before auto-repacking.
configitem('treedirstate', 'repackfactor', default=3)

class _reader(object):
    def __init__(self, data, offset):
        self.data = data
        self.offset = offset

    def readuint(self):
        v = struct.unpack(">L", self.data[self.offset:self.offset + 4])
        self.offset += 4
        return v[0]

    def readstr(self):
        l = self.readuint()
        v = self.data[self.offset:self.offset + l]
        self.offset += l
        return v

class _writer(object):
    def __init__(self):
        self.buffer = pycompat.stringio()

    def writeuint(self, v):
        self.buffer.write(struct.pack(">L", v))

    def writestr(self, v):
        self.writeuint(len(v))
        self.buffer.write(v)

class _overlaydict(dict):
    def __init__(self, lookup, *args, **kwargs):
        super(_overlaydict, self).__init__(*args, **kwargs)
        self.lookup = lookup

    def get(self, key, default=None):
        s = super(_overlaydict, self)
        if s.__contains__(key):
            return s.__getitem__(key)
        r = self.lookup(key)
        if r is not None:
            return r
        return default

    def __getitem__(self, key):
        s = super(_overlaydict, self)
        if s.__contains__(key):
            return s[key]
        r = self.lookup(key)
        if r is not None:
            return r
        raise KeyError(key)

# The treedirstatemap iterator uses the getnext method on the dirstatemap
# to find the next item on each call.  This involves searching down the
# tree each time.  A future improvement is to keep the state between each
# call to avoid these extra searches.
class treedirstatemapiterator(object):
    def __init__(self, map_, removed=False):
        self._rmap = map_
        self._removed = removed
        self._at = None

    def __iter__(self):
        return self

    def __next__(self):
        nextitem = self._rmap.getnext(self._at, self._removed)
        if nextitem is None:
            raise StopIteration
        self._at = nextitem[0]
        return nextitem

    def next(self):
        return self.__next__()

class treedirstatemap(object):
    def __init__(self, ui, opener, root, importmap=None):
        self._ui = ui
        self._opener = opener
        self._root = root
        self.copymap = {}

        self._filename = 'dirstate'
        self._rmap = rusttreedirstate.treedirstatemap(ui, opener)
        self._treeid = None
        self._parents = None
        self._dirtyparents = False
        self._nonnormalset = set()
        self._otherparentset = set()
        self._packedsize = 0

        if importmap is not None:
            self._rmap.importmap(importmap)
            self._parents = importmap._parents
            self._nonnormalset = importmap.nonnormalset
            self._otherparentset = importmap.otherparentset
            self.copymap = importmap.copymap
        else:
            self.read()

    def preload(self):
        pass

    def clear(self):
        self._rmap.clear()
        self.copymap.clear()
        self._nonnormalset.clear()
        self._otherparentset.clear()
        self.setparents(node.nullid, node.nullid)
        util.clearcachedproperty(self, "filefoldmap")
        util.clearcachedproperty(self, "dirfoldmap")

    def __len__(self):
        """Returns the number of files, including removed files."""
        return self._rmap.filecount()

    def itertrackeditems(self):
        """Returns an iterator over (filename, (state, mode, size, mtime))."""
        return treedirstatemapiterator(self._rmap, removed=False)

    def iterremoveditems(self):
        """
        Returns an iterator over (filename, (state, mode, size, mtime)) for
        files that have been marked as removed.
        """
        return treedirstatemapiterator(self._rmap, removed=True)

    def iteritems(self):
        return itertools.chain(self.itertrackeditems(),
                               self.iterremoveditems())

    def gettracked(self, filename, default=None):
        """Returns (state, mode, size, mtime) for the tracked file."""
        return self._rmap.gettracked(filename, default)

    def getremoved(self, filename, default=None):
        """Returns (state, mode, size, mtime) for the removed file."""
        return self._rmap.getremoved(filename, default)

    def get(self, filename, default=None):
        return (self._rmap.gettracked(filename, None) or
                self._rmap.getremoved(filename, default))

    def getcasefoldedtracked(self, filename, foldfunc):
        return self._rmap.getcasefoldedtracked(filename, foldfunc, id(foldfunc))

    def __getitem__(self, filename):
        item = (self._rmap.gettracked(filename, None) or
                self._rmap.getremoved(filename, None))
        if item is None:
            raise KeyError(filename)
        return item

    def hastrackedfile(self, filename):
        """Returns true if the file is tracked in the dirstate."""
        return self._rmap.hastrackedfile(filename)

    def hasremovedfile(self, filename):
        """Returns true if the file is recorded as removed in the dirstate."""
        return self._rmap.hasremovedfile(filename)

    def __contains__(self, filename):
        return (self._rmap.hastrackedfile(filename) or
                self._rmap.hasremovedfile(filename))

    def trackedfiles(self):
        """Returns a list of all filenames tracked by the dirstate."""
        trackedfiles = []
        self._rmap.visittrackedfiles(trackedfiles.append)
        return iter(trackedfiles)

    def removedfiles(self):
        """Returns a list of all removed files in the dirstate."""
        removedfiles = []
        self._rmap.visitremovedfiles(removedfiles.append)
        return removedfiles

    def __iter__(self):
        """Returns an iterator of all files in the dirstate."""
        trackedfiles = self.trackedfiles()
        removedfiles = self.removedfiles()
        if removedfiles:
            return heapq.merge(iter(trackedfiles), iter(removedfiles))
        else:
            return iter(trackedfiles)

    def keys(self):
        return list(iter(self))

    def hastrackeddir(self, dirname):
        """
        Returns True if the dirstate includes a directory.
        """
        return self._rmap.hastrackeddir(dirname + '/')

    def hasremoveddir(self, dirname):
        """
        Returns True if the directories containing files marked for removal
        includes a directory.
        """
        return self._rmap.hasremoveddir(dirname + '/')

    def hasdir(self, dirname):
        """
        Returns True if the directory exists in the dirstate for either
        tracked or removed files.
        """
        return self.hastrackeddir(dirname) or self.hasremoveddir(dirname)

    def addfile(self, f, oldstate, state, mode, size, mtime):
        self._rmap.addfile(f, oldstate, state, mode, size, mtime)
        if self._nonnormalset is not None:
            if state != 'n' or mtime == -1:
                self._nonnormalset.add(f)
            else:
                self._nonnormalset.discard(f)
        if self._otherparentset is not None:
            if size == -2:
                self._otherparentset.add(f)
            else:
                self._otherparentset.discard(f)

    def removefile(self, f, oldstate, size):
        self._rmap.removefile(f, oldstate, size)
        if self._nonnormalset is not None:
            self._nonnormalset.add(f)
        if size == -2 and self._otherparentset is not None:
            self._otherparentset.add(f)

    def dropfile(self, f, oldstate):
        """
        Drops a file from the dirstate.  Returns True if it was previously
        recorded.
        """
        if self._nonnormalset is not None:
            self._nonnormalset.discard(f)
        if self._otherparentset is not None:
            self._otherparentset.discard(f)
        return self._rmap.dropfile(f)

    def clearambiguoustimes(self, files, now):
        """Mark files with an mtime of `now` as being out of date.

        See mercurial/pure/parsers.py:pack_dirstate in core Mercurial for why
        this is done.
        """
        for f in files:
            e = self.gettracked(f)
            if e is not None and e[0] == 'n' and e[3] == now:
                self._rmap.addfile(f, e[0], e[0], e[1], e[2], -1)
                self.nonnormalset.add(f)

    def parents(self):
        """
        Returns the parents of the dirstate.
        """
        return self._parents

    def setparents(self, p1, p2):
        """
        Sets the dirstate parents.
        """
        self._parents = (p1, p2)
        self._dirtyparents = True

    def _computenonnormals(self):
        self._nonnormalset = set()
        self._otherparentset = set()
        self._rmap.computenonnormals(self._nonnormalset.add,
                                     self._otherparentset.add)

    @property
    def nonnormalset(self):
        if self._nonnormalset is None:
            self._computenonnormals()
        return self._nonnormalset

    @property
    def otherparentset(self):
        if self._otherparentset is None:
            self._computenonnormals()
        return self._otherparentset

    @util.propertycache
    def filefoldmap(self):
        """Returns a dictionary mapping normalized case paths to their
        non-normalized versions.
        """
        def lookup(key):
            f = self.getcasefoldedtracked(key, util.normcase)
            if f is not None and self._rmap.hastrackedfile(f):
                return f
            else:
                return None
        return _overlaydict(lookup)

    @util.propertycache
    def dirfoldmap(self):
        """
        Returns a dictionary mapping normalized case paths to their
        non-normalized versions for directories.
        """
        def lookup(key):
            d = self.getcasefoldedtracked(key + '/', util.normcase)
            if d is not None and self._rmap.hastrackeddir(d):
                return d.rstrip('/')
            else:
                return None
        return _overlaydict(lookup)

    @property
    def identity(self):
        if self._identity is None:
            self.read()
        return self._identity

    def _opendirstatefile(self):
        fp, _mode = txnutil.trypending(self._root, self._opener, self._filename)
        return fp

    def read(self):
        # ignore HG_PENDING because identity is used only for writing
        self._identity = util.filestat.frompath(
            self._opener.join(self._filename))

        try:
            data = self._opendirstatefile().read()
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            # File doesn't exist so current state is empty.
            if not self._dirtyparents:
                self._parents = (node.nullid, node.nullid)
            return

        if data[40:80] != dirstateheader:
            raise error.Abort(_('dirstate is not a valid treedirstate'))

        if not self._dirtyparents:
            self._parents = data[:20], data[20:40]

        r = _reader(data, 80)

        version = r.readuint()
        if version != treedirstateversion:
            raise error.Abort(_('unsupported treedirstate version: %s')
                              % version)

        self._treeid = r.readstr()
        rootid = r.readuint()
        self._packedsize = r.readuint()
        self._rmap.read(treefileprefix + self._treeid, rootid)
        clen = r.readuint()
        copymap = {}
        for _i in range(clen):
            k = r.readstr()
            v = r.readstr()
            copymap[k] = v

        def readset():
            slen = r.readuint()
            if slen == setabsent:
                return None
            s = set()
            for _i in range(slen):
                s.add(r.readstr())
            return s

        nonnormalset = readset()
        otherparentset = readset()

        self.copymap = copymap
        self._nonnormalset = nonnormalset
        self._otherparentset = otherparentset

    def startwrite(self, tr):
        # TODO: register map store offset with 'tr'
        pass

    def write(self, st, now):
        """Write the dirstate to the filehandle st."""
        if self._nonnormalset is not None:
            nonnormadd = self._nonnormalset.add
        else:
            def nonnormadd(f):
                pass
        repackfactor = self._ui.configint('treedirstate', 'repackfactor')
        minrepackthreshold = self._ui.configint('treedirstate',
                                                'minrepackthreshold')
        repackthreshold = max(self._packedsize * repackfactor,
                              minrepackthreshold)
        if self._rmap.storeoffset() > repackthreshold:
            self._ui.note(_("auto-repacking treedirstate\n"))
            self._ui.log('treedirstate_repacking', '',
                         treedirstate_repacking=True)
            self._repacked = True
            self._treeid = None
        else:
            self._extended = True
        if self._treeid is None:
            self._treeid = newtree(self._opener)
            self._rmap.write(treefileprefix + self._treeid, now, nonnormadd)
            self._packedsize = self._rmap.storeoffset()
        else:
            self._rmap.writedelta(now, nonnormadd)
        st.write(self._genrootdata())
        st.close()
        if self._ui.configbool('treedirstate', 'verify'):
            self._verify()
        self._dirtyparents = False

    def writeflat(self):
        with self._opener("dirstate", "w",
                          atomictemp=True, checkambig=True) as st:
            newdmap = {}
            for k, v in self.iteritems():
                newdmap[k] = dirstate.dirstatetuple(*v)

            st.write(dirstate.parsers.pack_dirstate(
                newdmap, self.copymap, self._parents,
                dirstate._getfsnow(self._opener)))

    def _verify(self):
        # Re-open the treedirstate to check it's ok
        rootid = self._rmap.rootid()
        try:
            self._ui.debug('reopening %s with root %s to check it\n'
                           % (treefileprefix + self._treeid, rootid))
            self._rmap.read(treefileprefix + self._treeid, rootid)
        except Exception as e:
            self._ui.warn(_('error verifying treedirstate after update: %s\n')
                          % e)
            self._ui.warn(_('please post the following debug information '
                            'to the Source Control @ FB group:\n'))
            treestat = self._opener.lstat(treefileprefix + self._treeid)
            self._ui.warn(_('rootid: %s, treefile: %s, treestat: %s, now: %s\n')
                          % (rootid, treefileprefix + self._treeid,
                             treestat, time.time()))
            with self._opener(treefileprefix + self._treeid, 'rb') as f:
                f.seek(-256, 2)
                pos = f.tell()
                data = f.read(32)
                while data:
                    self._ui.warn(('%08x: %s\n')
                                  % (pos, binascii.hexlify(data)))
                    pos = f.tell()
                    data = f.read(32)
            raise error.Abort(_('error verifying treedirstate'))

    def _genrootdata(self):
        w = _writer()
        if self._parents:
            w.buffer.write(self._parents[0])
            w.buffer.write(self._parents[1])
        else:
            w.buffer.write(node.nullid)
            w.buffer.write(node.nullid)
        w.buffer.write(dirstateheader)
        w.writeuint(treedirstateversion)
        w.writestr(self._treeid)
        w.writeuint(self._rmap.rootid())
        w.writeuint(self._packedsize)
        w.writeuint(len(self.copymap))
        for k, v in self.copymap.iteritems():
            w.writestr(k)
            w.writestr(v)

        setthreshold = max(1000, self._rmap.filecount() / 3)

        def writeset(s):
            if s is None or len(s) > setthreshold:
                # The set is absent or too large.  Mark it as absent.
                w.writeuint(setabsent)
            else:
                w.writeuint(len(s))
                for v in s:
                    w.writestr(v)

        writeset(self._nonnormalset)
        writeset(self._otherparentset)

        return w.buffer.getvalue()

def istreedirstate(repo):
    requirements = getattr(repo, 'requirements', ())
    # Eden has its own dirstate implementation
    if 'eden' in requirements:
        return False
    return 'treedirstate' in requirements

def activealternativedirstates(repo):
    """
    Returns a set containing the names of any alternative dirstate
    implementations in use.
    """
    alternatives = {'eden', 'sqldirstate'}
    requirements = getattr(repo, 'requirements', set())
    return alternatives & requirements

def newtree(opener):
    while True:
        treeid = ''.join([random.choice(string.digits) for _c in range(8)])
        if not opener.exists(treefileprefix + treeid):
            return treeid

def gettreeid(opener, dirstatefile):
    # The treeid is located within the first 128 bytes.
    with opener(dirstatefile) as fp:
        data = fp.read(128)
    if data[40:80] != dirstateheader:
        return None
    r = _reader(data, 80)
    version = r.readuint()
    if version != treedirstateversion:
        return None
    return r.readstr()

def upgrade(ui, repo):
    if istreedirstate(repo):
        raise error.Abort('repo already has treedirstate')
    alternatives = activealternativedirstates(repo)
    if alternatives:
        raise error.Abort('repo has alternative dirstate active: %s'
                          % ', '.join(alternatives))
    with repo.wlock():
        newmap = treedirstatemap(ui, repo.dirstate._opener, repo.root,
                                 importmap=repo.dirstate._map)
        f = repo.dirstate._opener('dirstate', 'w')
        newmap.write(f, dirstate._getfsnow(repo.dirstate._opener))
        repo.requirements.add('treedirstate')
        repo._writerequirements()
        del repo.dirstate

def downgrade(ui, repo):
    if not istreedirstate(repo):
        raise error.Abort('repo doesn\'t have treedirstate')
    with repo.wlock():
        repo.dirstate._map.writeflat()
        repo.requirements.remove('treedirstate')
        repo._writerequirements()
        del repo.dirstate

def repack(ui, repo):
    if not istreedirstate(repo):
        ui.note(_("not repacking because repo does not have treedirstate"))
        return
    with repo.wlock():
        repo.dirstate._map._treeid = None
        repo.dirstate._dirty = True

dirstatefiles = [
    'dirstate',
    'dirstate.pending',
    'undo.dirstate',
    'undo.backup.dirstate',
]

def cleanup(ui, repo):
    """Clean up old tree files.

    When repacking, we write out the tree data to a new file.  This allows us
    to rollback transactions without fear of losing dirstate information, as
    the old dirstate file points at the old tree file.

    This leaves old tree files lying around.  We must periodically clean up
    any tree files that are not referred to by any of the dirstate files.
    """
    with repo.wlock():
        treesinuse = {}
        for f in dirstatefiles:
            try:
                treeid = gettreeid(repo.vfs, f)
                if treeid is not None:
                    treesinuse.setdefault(treeid, set()).add(f)
            except Exception:
                pass
        for f in repo.vfs.listdir():
            if f.startswith(treefileprefix):
                treeid = f[len(treefileprefix):]
                if treeid not in treesinuse:
                    ui.debug("dirstate tree %s unused, deleting\n" % treeid)
                    repo.vfs.unlink(f)
                else:
                    ui.debug("dirstate tree %s in use by %s\n"
                             % (treeid, ', '.join(treesinuse[treeid])))

def wrapdirstate(orig, self):
    ds = orig(self)
    if istreedirstate(self):
        ds._mapcls = treedirstatemap
    return ds

class casecollisionauditor(object):
    def __init__(self, ui, abort, dirstate):
        self._ui = ui
        self._abort = abort
        self._dirstate = dirstate
        # The purpose of _newfiles is so that we don't complain about
        # case collisions if someone were to call this object with the
        # same filename twice.
        self._newfiles = set()
        self._newfilesfolded = set()

    def __call__(self, f):
        if f in self._newfiles:
            return
        fl = encoding.lower(f)
        if (f not in self._dirstate and
                (fl in self._newfilesfolded or
                 self._dirstate._map.getcasefoldedtracked(fl, encoding.lower))):
            msg = _('possible case-folding collision for %s') % f
            if self._abort:
                raise error.Abort(msg)
            self._ui.warn(_("warning: %s\n") % msg)
        self._newfiles.add(f)
        self._newfilesfolded.add(fl)

def wrapcca(orig, ui, abort, dirstate):
    if util.safehasattr(dirstate._map, 'getcasefoldedtracked'):
        return casecollisionauditor(ui, abort, dirstate)
    else:
        return orig(ui, abort, dirstate)

def wrapclose(orig, self):
    """
    Wraps repo.close to perform cleanup of old dirstate tree files.  This
    happens whenever the treefile is repacked, and also on 1% of other
    invocations that involve writing to treedirstate.
    """
    # For chg, do not clean up on the "serve" command
    if 'CHGINTERNALMARK' in encoding.environ:
        return orig(self)

    try:
        return orig(self)
    finally:
        istreedirstate = ("_map" in self.dirstate.__dict__ and
                          isinstance(self.dirstate._map, treedirstatemap))
        if istreedirstate:
            haverepacked = getattr(self.dirstate._map, "_repacked", False)
            haveextended = getattr(self.dirstate._map, "_extended", False)
            cleanuppercent = self.ui.configint('treedirstate', 'cleanuppercent')
            if (haverepacked or
                    (haveextended and random.randint(0, 99) < cleanuppercent)):
                # We have written to the dirstate as part of this command, so
                # cleaning up should also be able to write to the repo.
                cleanup(self.ui, self)

def wrapnewreporequirements(orig, repo):
    reqs = orig(repo)
    if repo.ui.configbool('treedirstate', 'useinnewrepos'):
        reqs.add('treedirstate')
    return reqs

def wrappull(orig, ui, repo, *args, **kwargs):
    if (ui.configbool('treedirstate', 'downgradeonpull') and
            istreedirstate(repo)):
        ui.status(_('disabling treedirstate...\n'))
        downgrade(ui, repo)
    elif (ui.configbool('treedirstate', 'upgradeonpull') and
            not istreedirstate(repo) and not activealternativedirstates(repo)):
        ui.status(_('please wait while we migrate your repo to treedirstate\n'
                    'this will make your hg commands faster...\n'))
        upgrade(ui, repo)

    return orig(ui, repo, *args, **kwargs)

def wrapdebugpathcomplete(orig, ui, repo, *specs, **opts):
    if istreedirstate(repo):
        cwd = repo.getcwd()
        matches = []
        rootdir = repo.root + pycompat.ossep
        acceptable = ''
        if opts[r'normal']:
            acceptable += 'nm'
        if opts[r'added']:
            acceptable += 'a'
        if opts[r'removed']:
            acceptable += 'r'
        if not acceptable:
            acceptable = 'nmar'
        fullpaths = bool(opts[r'full'])
        fixpaths = pycompat.ossep != '/'
        treedirstatemap = repo.dirstate._map._rmap
        for spec in sorted(specs) or ['']:
            spec = os.path.normpath(os.path.join(pycompat.getcwd(), spec))
            if spec != repo.root and not spec.startswith(rootdir):
                continue
            if os.path.isdir(spec):
                spec += '/'
            spec = spec[len(rootdir):]
            if fixpaths:
                spec = spec.replace(pycompat.ossep, '/')
            treedirstatemap.pathcomplete(spec, acceptable, matches.append,
                                         fullpaths)
        for p in matches:
            p = repo.pathto(p, cwd).rstrip('/')
            if fixpaths:
                p = p.replace('/', pycompat.ossep)
            ui.write(p)
            ui.write('\n')
    else:
        return orig(ui, repo, *specs, **opts)

def featuresetup(ui, supported):
    supported |= {'treedirstate'}

def extsetup(ui):
    # Check this version of Mercurial has the extension points we need
    if not util.safehasattr(dirstate.dirstatemap, "hasdir"):
        ui.warn(_("this version of Mercurial doesn't support treedirstate\n"))
        return

    if util.safehasattr(localrepo, 'newreporequirements'):
        extensions.wrapfunction(localrepo, 'newreporequirements',
                                wrapnewreporequirements)

    localrepo.localrepository.featuresetupfuncs.add(featuresetup)
    extensions.wrapfilecache(localrepo.localrepository, 'dirstate',
                             wrapdirstate)
    extensions.wrapfunction(scmutil, 'casecollisionauditor', wrapcca)
    extensions.wrapfunction(localrepo.localrepository, 'close', wrapclose)
    extensions.wrapcommand(commands.table, 'pull', wrappull)
    extensions.wrapcommand(commands.table, 'debugpathcomplete',
                           wrapdebugpathcomplete)

def reposetup(ui, repo):
    ui.log('treedirstate_enabled', '',
           treedirstate_enabled=istreedirstate(repo))

# debug commands
cmdtable = {}
command = registrar.command(cmdtable)

@command('debugtreedirstate', [],
         'hg debugtreedirstate [on|off|status|repack|cleanup]')
def debugtreedirstate(ui, repo, cmd, **opts):
    """manage treedirstate"""
    if cmd == "on":
        upgrade(ui, repo)
    elif cmd == "off":
        downgrade(ui, repo)
        cleanup(ui, repo)
    elif cmd == "repack":
        repack(ui, repo)
        cleanup(ui, repo)
    elif cmd == "cleanup":
        cleanup(ui, repo)
    elif cmd == "status":
        if istreedirstate(repo):
            ui.status(_("treedirstate enabled " +
                        "(using dirstate.tree.%s, %s files tracked)")
                      % (repo.dirstate._map._treeid, len(repo.dirstate._map)))
        else:
            ui.status(_("treedirstate not enabled"))
    else:
        raise error.Abort("unrecognised command: %s" % cmd)
