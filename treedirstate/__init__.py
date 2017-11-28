# Copyright Facebook, Inc. 2017
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""tree-based dirstate implementation"""

from __future__ import absolute_import
from mercurial import (
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
from mercurial.i18n import _
import errno
import heapq
import itertools
import struct

from .rusttreedirstate import RustDirstateMap

dirstateheader = b'########################treedirstate####'
treedirstateversion = 1
useinnewrepos = True

# Sentinel length value for when a nonnormalset or otherparentset is absent.
setabsent = 0xffffffff

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
        self._rmap = RustDirstateMap(ui, opener)
        self._treeid = None
        self._parents = None
        self._dirtyparents = False
        self._nonnormalset = set()
        self._otherparentset = set()

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
        return self._rmap.getcasefoldedtracked(filename, foldfunc)

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
        return self.hastrackedfile(filename) or self.hasremovedfile(filename)

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

    def getfilefoldmap(self):
        """Returns a dictionary mapping normalized case paths to their
        non-normalized versions.
        """
        raise NotImplementedError()

    def getdirfoldmap(self):
        """
        Returns a dictionary mapping normalized case paths to their
        non-normalized versions for directories.
        """
        raise NotImplementedError()

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
        self._rmap.read('dirstate.tree.000', rootid)
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
        if self._treeid is None:
            self._treeid = '000'
            self._rmap.write('dirstate.tree.000', now, nonnormadd)
        else:
            self._rmap.writedelta(now, nonnormadd)
        st.write(self._genrootdata())
        st.close()
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
    return 'treedirstate' in getattr(repo, 'requirements', set())

def upgrade(ui, repo):
    if istreedirstate(repo):
        raise error.Abort('repo already has treedirstate')
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

def wrapnewreporequirements(orig, repo):
    reqs = orig(repo)
    if useinnewrepos:
        reqs.add('treedirstate')
    return reqs

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

def reposetup(ui, repo):
    ui.log('treedirstate_enabled', '',
           treedirstate_enabled=istreedirstate(repo))

# debug commands
cmdtable = {}
command = registrar.command(cmdtable)

@command('debugtreedirstate', [], 'hg debugtreedirstate [on|off|status]')
def debugtreedirstate(ui, repo, cmd, **opts):
    """migrate to treedirstate"""
    if cmd == "on":
        upgrade(ui, repo)
    elif cmd == "off":
        downgrade(ui, repo)
    elif cmd == "status":
        if istreedirstate(repo):
            ui.status(_("treedirstate enabled " +
                        "(using dirstate.tree.%s, %s files tracked)")
                      % (repo.dirstate._map._treeid, len(repo.dirstate._map)))
        else:
            ui.status(_("treedirstate not enabled"))
    else:
        raise error.Abort("unrecognised command: %s" % cmd)
