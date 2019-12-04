# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""tree-based dirstate"""

from __future__ import absolute_import

import binascii
import errno
import heapq
import itertools
import random
import string
import struct
import time

# pyre-fixme[21]: Could not find `bindings`.
from bindings import treestate as rusttreestate

from . import error, node, pycompat, treestate, txnutil, util
from .i18n import _


dirstateheader = b"########################treedirstate####"
treedirstateversion = 1
treefileprefix = "dirstate.tree."

# Sentinel length value for when a nonnormalset or otherparentset is absent.
setabsent = 0xFFFFFFFF


class _reader(object):
    def __init__(self, data, offset):
        self.data = data
        self.offset = offset

    def readuint(self):
        v = struct.unpack(">L", self.data[self.offset : self.offset + 4])
        self.offset += 4
        return v[0]

    def readstr(self):
        l = self.readuint()
        v = self.data[self.offset : self.offset + l]
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

        self._filename = "dirstate"
        self._rmap = rusttreestate.treedirstatemap(ui, opener)
        self._treeid = None
        self._parents = None
        self._dirtyparents = False
        self._nonnormalset = set()
        self._otherparentset = set()
        self._packedsize = 0

        if importmap is not None:
            self._rmap.importmap(importmap)
            self._parents = importmap._parents

            def shouldtrack(filename):
                return self._rmap.hastrackedfile(filename) or self._rmap.hasremovedfile(
                    filename
                )

            self._nonnormalset = set(filter(shouldtrack, importmap.nonnormalset))
            self._otherparentset = set(filter(shouldtrack, importmap.otherparentset))
            self.copymap = {
                dst: src for dst, src in importmap.copymap.items() if shouldtrack(dst)
            }
        else:
            self.read()

    def preload(self):
        pass

    def clear(self):
        self._rmap.clear()
        self.copymap.clear()
        if self._nonnormalset is not None:
            self._nonnormalset.clear()
        if self._otherparentset is not None:
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
        return itertools.chain(self.itertrackeditems(), self.iterremoveditems())

    def gettracked(self, filename, default=None):
        """Returns (state, mode, size, mtime) for the tracked file."""
        return self._rmap.gettracked(filename, default)

    def getremoved(self, filename, default=None):
        """Returns (state, mode, size, mtime) for the removed file."""
        return self._rmap.getremoved(filename, default)

    def get(self, filename, default=None):
        return self._rmap.gettracked(filename, None) or self._rmap.getremoved(
            filename, default
        )

    def getcasefoldedtracked(self, filename, foldfunc):
        return self._rmap.getcasefoldedtracked(filename, foldfunc, id(foldfunc))

    def getfiltered(self, filename, foldfunc):
        f = self.getcasefoldedtracked(filename, foldfunc)
        return [f] if f else []

    def __getitem__(self, filename):
        item = self._rmap.gettracked(filename, None) or self._rmap.getremoved(
            filename, None
        )
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
        return self._rmap.hastrackedfile(filename) or self._rmap.hasremovedfile(
            filename
        )

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
        return self._rmap.hastrackeddir(dirname + "/")

    def hasremoveddir(self, dirname):
        """
        Returns True if the directories containing files marked for removal
        includes a directory.
        """
        return self._rmap.hasremoveddir(dirname + "/")

    def hasdir(self, dirname):
        """
        Returns True if the directory exists in the dirstate for either
        tracked or removed files.
        """
        return self.hastrackeddir(dirname) or self.hasremoveddir(dirname)

    def addfile(self, f, oldstate, state, mode, size, mtime):
        self._rmap.addfile(f, oldstate, state, mode, size, mtime)
        if self._nonnormalset is not None:
            if state != "n" or mtime == -1:
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

    def untrackfile(self, f, oldstate):
        """
        Drops a file from the dirstate.  Returns True if it was previously
        recorded.
        """
        if self._nonnormalset is not None:
            self._nonnormalset.discard(f)
        if self._otherparentset is not None:
            self._otherparentset.discard(f)
        return self._rmap.untrackfile(f)

    def deletefile(self, f, oldstate):
        """
        Drops a file from the dirstate entirely, as if it was deleted from disk.
        Useful for informing the map that it doesn't need to keep a record of
        this file for future checking.

        In the treedirstate implementation, it is the same as untrackfile.
        """
        return self.untrackfile(f, oldstate)

    def clearambiguoustimes(self, files, now):
        """Mark files with an mtime of `now` as being out of date.

        See mercurial/pure/parsers.py:pack_dirstate in core Mercurial for why
        this is done.
        """
        for f in files:
            e = self.gettracked(f)
            if e is not None and e[0] == "n" and e[3] == now:
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
        self._rmap.computenonnormals(self._nonnormalset.add, self._otherparentset.add)

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

        return treestate._overlaydict(lookup)

    @util.propertycache
    def dirfoldmap(self):
        """
        Returns a dictionary mapping normalized case paths to their
        non-normalized versions for directories.
        """

        def lookup(key):
            d = self.getcasefoldedtracked(key + "/", util.normcase)
            if d is not None and self._rmap.hastrackeddir(d):
                return d.rstrip("/")
            else:
                return None

        return treestate._overlaydict(lookup)

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
        self._identity = util.filestat.frompath(self._opener.join(self._filename))

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
            raise error.Abort(_("dirstate is not a valid treedirstate"))

        if not self._dirtyparents:
            self._parents = data[:20], data[20:40]

        r = _reader(data, 80)

        version = r.readuint()
        if version != treedirstateversion:
            raise error.Abort(_("unsupported treedirstate version: %s") % version)

        self._treeid = r.readstr()
        rootid = r.readuint()
        self._packedsize = r.readuint()
        self._ui.log(
            "treedirstate", "loading tree %r rootid %r" % (self._treeid, rootid)
        )
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

        repackfactor = self._ui.configint("treestate", "repackfactor")
        minrepackthreshold = self._ui.configbytes("treestate", "minrepackthreshold")
        repackthreshold = max(self._packedsize * repackfactor, minrepackthreshold)
        if self._rmap.storeoffset() > repackthreshold:
            self._ui.note(_("auto-repacking treedirstate\n"))
            self._ui.log("treedirstate_repacking", treedirstate_repacking=True)
            self._repacked = True
            self._treeid = None
            self._gc()
        if self._treeid is None:
            self._treeid = newtree(self._opener)
            self._rmap.write(treefileprefix + self._treeid, now, nonnormadd)
            self._packedsize = self._rmap.storeoffset()
        else:
            self._rmap.writedelta(now, nonnormadd)
        st.write(self._genrootdata())
        st.close()
        self._dirtyparents = False

    def writeflat(self):
        from edenscm.mercurial import dirstate

        with self._opener("dirstate", "w", atomictemp=True, checkambig=True) as st:
            newdmap = {}
            for k, v in self.iteritems():
                newdmap[k] = dirstate.dirstatetuple(*v)

            st.write(
                dirstate.parsers.pack_dirstate(
                    newdmap,
                    self.copymap,
                    self._parents,
                    dirstate._getfsnow(self._opener),
                )
            )

    def _verify(self):
        # Re-open the treedirstate to check it's ok
        rootid = self._rmap.rootid()
        try:
            self._ui.debug(
                "reopening %s with root %s to check it\n"
                % (treefileprefix + self._treeid, rootid)
            )
            self._rmap.read(treefileprefix + self._treeid, rootid)
        except Exception as e:
            self._ui.warn(_("error verifying treedirstate after update: %s\n") % e)
            self._ui.warn(
                _(
                    "please post the following debug information "
                    "to the Source Control @ FB group:\n"
                )
            )
            treestat = self._opener.lstat(treefileprefix + self._treeid)
            self._ui.warn(
                _("rootid: %s, treefile: %s, treestat: %s, now: %s\n")
                % (rootid, treefileprefix + self._treeid, treestat, time.time())
            )
            with self._opener(treefileprefix + self._treeid, "rb") as f:
                f.seek(-256, 2)
                pos = f.tell()
                data = f.read(32)
                while data:
                    self._ui.warn(("%08x: %s\n") % (pos, binascii.hexlify(data)))
                    pos = f.tell()
                    data = f.read(32)
            raise error.Abort(_("error verifying treedirstate"))

    def _gc(self):
        """Clean up old tree files.

        When repacking, we write out the tree data to a new file.  This allows us
        to rollback transactions without fear of losing dirstate information, as
        the old dirstate file points at the old tree file.

        This leaves old tree files lying around.  We must periodically clean up
        any tree files that are not referred to by any of the dirstate files.
        """
        treesinuse = {}
        for f in ["dirstate", "undo.dirstate", "undo.backup.dirstate"]:
            try:
                treeid = gettreeid(self._opener, f)
                if treeid is not None:
                    treesinuse.setdefault(treeid, set()).add(f)
            except Exception:
                pass
        from . import dirstate  # avoid cycle

        fsnow = dirstate._getfsnow(self._opener)
        maxmtime = fsnow - self._ui.configint("treestate", "mingcage")
        for f in self._opener.listdir():
            if f.startswith(treefileprefix):
                treeid = f[len(treefileprefix) :]
                if treeid in treesinuse:
                    self._ui.debug(
                        "dirstate tree %s in use by %s\n"
                        % (treeid, ", ".join(treesinuse[treeid]))
                    )
                    continue
                try:
                    if self._opener.stat(f).st_mtime > maxmtime:
                        continue
                except OSError:
                    continue
                self._ui.debug("removing old unreferenced dirstate tree %s\n" % treeid)
                self._opener.tryunlink(f)

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


def newtree(opener):
    while True:
        treeid = "".join([random.choice(string.digits) for _c in range(8)])
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
