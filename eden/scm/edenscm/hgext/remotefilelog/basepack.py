# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import collections
import errno
import hashlib
import os
import stat as statmod
import struct
import time

from edenscm.mercurial import error, policy, pycompat, util, vfs as vfsmod
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import range

from . import constants, shallowutil


# The amount of time to wait between checking for new packs. This prevents an
# exception when data is moved to a new pack after the process has already
# loaded the pack list.
REFRESHRATE = 0.1

if pycompat.isposix:
    # With glibc 2.7+ the 'e' flag uses O_CLOEXEC when opening.
    # The 'e' flag will be ignored on older versions of glibc.
    PACKOPENMODE = "rbe"
else:
    PACKOPENMODE = "rb"


class _cachebackedpacks(object):
    def __init__(self, packs, cachesize):
        self._packs = set(packs)
        self._lrucache = util.lrucachedict(cachesize)
        self._lastpack = None

        # Avoid cold start of the cache by populating the most recent packs
        # in the cache.
        for i in reversed(range(min(cachesize, len(packs)))):
            self._movetofront(packs[i])

    def __len__(self):
        return len(self._lrucache)

    def _movetofront(self, pack):
        # This effectively makes pack the first entry in the cache.
        self._lrucache[pack] = True

    def _registerlastpackusage(self):
        if self._lastpack is not None:
            self._movetofront(self._lastpack)
            self._lastpack = None

    def add(self, pack):
        self._registerlastpackusage()

        # This method will mostly be called when packs are not in cache.
        # Therefore, adding pack to the cache.
        self._movetofront(pack)
        self._packs.add(pack)

    def remove(self, pack):
        self._packs.remove(pack)
        del self._lrucache[pack]

    def __iter__(self):
        self._registerlastpackusage()

        # Cache iteration is based on LRU.
        for pack in self._lrucache:
            self._lastpack = pack
            yield pack

        if len(self._packs) != len(self._lrucache):
            cachedpacks = set(pack for pack in self._lrucache)
            # Yield for paths not in the cache.
            for pack in self._packs - cachedpacks:
                self._lastpack = pack
                yield pack

        # Data not found in any pack.
        self._lastpack = None

    def clear(self):
        self._packs.clear()
        self._lrucache.clear()
        self._lastpack = None


class basepackstore(object):
    # Default cache size limit for the pack files.
    DEFAULTCACHESIZE = 100

    def __init__(self, ui, path, deletecorruptpacks=False):
        self.ui = ui
        self.path = path
        self.deletecorruptpacks = deletecorruptpacks

        # lastrefesh is 0 so we'll immediately check for new packs on the first
        # failure.
        self.lastrefresh = 0

        self.packs = _cachebackedpacks([], self.DEFAULTCACHESIZE)
        self.packspath = set()

    def _getavailablepackfiles(self, currentpacks=None):
        """For each pack file (a index/data file combo), yields:
          (full path without extension, mtime, size)

        mtime will be the mtime of the index/data file (whichever is newer)
        size is the combined size of index/data file
        """
        if currentpacks is None:
            currentpacks = set()

        ids = set()
        sizes = collections.defaultdict(lambda: 0)
        mtimes = collections.defaultdict(lambda: [])
        try:
            for filename in os.listdir(self.path):
                filename = os.path.join(self.path, filename)
                id, ext = os.path.splitext(filename)

                if id not in currentpacks:
                    # Since we expect to have two files corresponding to each ID
                    # (the index file and the pack file), we can yield once we see
                    # it twice.
                    if ext == self.INDEXSUFFIX or ext == self.PACKSUFFIX:
                        st = util.lstat(filename)
                        if statmod.S_ISDIR(st.st_mode):
                            continue

                        sizes[id] += st.st_size  # Sum both files' sizes together
                        mtimes[id].append(st.st_mtime)
                        if id in ids:
                            yield (
                                os.path.join(self.path, id),
                                max(mtimes[id]),
                                sizes[id],
                            )
                        else:
                            ids.add(id)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise

    def _getavailablepackfilessorted(self, currentpacks):
        """Like `_getavailablepackfiles`, but also sorts the files by mtime,
        yielding newest files first.

        This is desirable, since it is more likely newer packfiles have more
        desirable data.
        """
        files = []
        for path, mtime, size in self._getavailablepackfiles(currentpacks):
            files.append((mtime, size, path))
        files = sorted(files, reverse=True)
        for __, __, path in files:
            yield path

    def gettotalsizeandcount(self):
        """Returns the total disk size (in bytes) of all the pack files in
        this store, and the count of pack files.

        (This might be smaller than the total size of the ``self.path``
        directory, since this only considers fuly-writen pack files, and not
        temporary files or other detritus on the directory.)
        """
        totalsize = 0
        count = 0
        for __, __, size in self._getavailablepackfiles():
            totalsize += size
            count += 1
        return totalsize, count

    def getmetrics(self):
        """Returns metrics on the state of this store."""
        size, count = self.gettotalsizeandcount()
        return {"numpacks": count, "totalpacksize": size}

    def getpack(self, path):
        raise NotImplemented()

    def getmissing(self, keys):
        missing = keys

        def func(pack):
            return pack.getmissing(missing)

        for newmissing in self.runonpacks(func):
            missing = newmissing
            if not missing:
                break

        return missing

    def markforrefresh(self):
        """Tells the store that there may be new pack files, so the next time it
        has a lookup miss it should check for new files."""
        self.lastrefresh = 0

    def refresh(self):
        """Checks for any new packs on disk, adds them to the main pack list,
        and returns a list of just the new packs."""
        now = time.time()

        # When remotefilelog.fetchpacks is enabled, some commands will trigger
        # many packfiles to be written to disk. This has the negative effect to
        # really slow down the refresh function, to the point where 90+% of the
        # time would be spent in it. A simple (but effective) solution is to
        # run repack when we detect that the number of packfiles is too big. A
        # better solution is to use a file format that isn't immutable, like
        # IndexedLog. Running repack is the short-time solution until
        # IndexedLog is more widely deployed.
        if len(self.packs) == self.DEFAULTCACHESIZE:
            self.packs.clear()
            self.packspath.clear()
            try:
                self.repackstore()
            except Exception:
                # Failures can happen due to concurrent repacks, which should
                # be rare. Let's just ignore these, the next refresh will
                # re-issue the repack and succeed.
                pass

        # If we experience a lot of misses (like in the case of getmissing() on
        # new objects), let's only actually check disk for new stuff every once
        # in a while. Generally this code path should only ever matter when a
        # repack is going on in the background, and that should be pretty rare
        # to have that happen twice in quick succession.
        newpacks = []
        if now > self.lastrefresh + REFRESHRATE:
            previous = self.packspath
            for filepath in self._getavailablepackfilessorted(previous):
                try:
                    newpack = self.getpack(filepath)
                    newpacks.append(newpack)
                except Exception as ex:
                    # An exception may be thrown if the pack file is corrupted
                    # somehow.  Log a warning but keep going in this case, just
                    # skipping this pack file.
                    #
                    # If this is an ENOENT error then don't even bother logging.
                    # Someone could have removed the file since we retrieved the
                    # list of paths.
                    if getattr(ex, "errno", None) != errno.ENOENT:
                        if self.deletecorruptpacks:
                            self.ui.warn(_("deleting corrupt pack '%s'\n") % filepath)
                            util.tryunlink(filepath + self.PACKSUFFIX)
                            util.tryunlink(filepath + self.INDEXSUFFIX)
                        else:
                            self.ui.warn(
                                _("detected corrupt pack '%s' - ignoring it\n")
                                % filepath
                            )

            self.lastrefresh = time.time()

        for pack in reversed(newpacks):
            self.packs.add(pack)
            self.packspath.add(pack.path())

        return newpacks

    def runonpacks(self, func):
        badpacks = []

        for pack in self.packs:
            try:
                yield func(pack)
            except KeyError:
                pass
            except Exception as ex:
                # Other exceptions indicate an issue with the pack file, so
                # remove it.
                badpacks.append((pack, getattr(ex, "errno", None)))

        newpacks = self.refresh()
        if newpacks != []:
            newpacks = set(newpacks)
            for pack in self.packs:
                if pack in newpacks:
                    try:
                        yield func(pack)
                    except KeyError:
                        pass
                    except Exception as ex:
                        # Other exceptions indicate an issue with the pack file, so
                        # remove it.
                        badpacks.append((pack, getattr(ex, "errno", None)))

        if badpacks:
            if self.deletecorruptpacks:
                for pack, err in badpacks:
                    self.packs.remove(pack)
                    self.packspath.remove(pack.path())

                    if err != errno.ENOENT:
                        self.ui.warn(_("deleting corrupt pack '%s'\n") % pack.path())
                        util.tryunlink(pack.packpath())
                        util.tryunlink(pack.indexpath())
            else:
                for pack, err in badpacks:
                    if err != errno.ENOENT:
                        self.ui.warn(
                            _("detected corrupt pack '%s' - ignoring it\n")
                            % pack.path()
                        )
