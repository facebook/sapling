# Copyright 2016-present Facebook. All Rights Reserved.
#
# linkrevcache: a simple caching layer to speed up _adjustlinkrev
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""a simple caching layer to speed up _adjustlinkrev

The linkrevcache extension memorizes some _adjustlinkrev results in a local
database in the directory '.hg/cache/linkrevdb'.
"""

import os
import shutil
import sys

from mercurial import (
    util,
)
_chosendbm = None

def _choosedbm():
    """return (name, module)"""
    global _chosendbm
    if not _chosendbm:
        if sys.version_info >= (3, 0):
            candidates = [('gdbm', 'dbm.gnu'), ('ndbm', 'dbm.ndbm'),
                          ('dumb', 'dbm.dumb')]
        else:
            candidates = [('gdbm', 'gdbm'), ('bsd', 'dbhash'),
                          ('ndbm', 'dbm'), ('dumb', 'dumbdbm')]
        for name, modname in candidates:
            try:
                mod = __import__(modname)
                mod.open  # sanity check with demandimport enabled
                _chosendbm = (name, __import__(modname))
                break
            except ImportError:
                pass
    return _chosendbm

# dbm is a bytes -> bytes map, so we need to convert integers to bytes.
# the conversion functions are optimized for space usage.
# not using struct.(un)pack is because we may have things > 4 bytes (revlog
# defines the revision number to be 6 bytes) and 8-byte is wasteful.

def _strinc(s):
    """return the "next" string. useful as an incremental "ID"."""
    if not s:
        # avoid '\0' so '\0' could be used as a separator
        return '\x01'
    n = ord(s[-1])
    if n == 255:
        return _strinc(s[:-1]) + '\x01'
    else:
        return s[:-1] + chr(n + 1)

def _str2int(s):
    # this is faster than "bytearray().extend(map(ord, s))"
    x = 0
    for ch in s:
        x <<= 8
        x += ord(ch)
    return x

def _int2str(x):
    s = ''
    while x:
        s = chr(x & 255) + s
        x >>= 8
    return s

def _intlist2str(intlist):
    result = ''
    for n in intlist:
        s = _int2str(n)
        l = len(s)
        # do not accept huge integers
        assert l < 256
        result += chr(l) + s
    return result

def _str2intlist(s):
    result = []
    i = 0
    end = len(s)
    while i < end:
        l = ord(s[i])
        i += 1
        result.append(_str2int(s[i:i + l]))
        i += l
    return result

class linkrevdbreadonly(object):
    _openflag = 'r'

    # numbers are useful in the atomic replace case: they can be sorted
    # and replaced in a safer order. however, atomic caller should always
    # use repo lock so the order only protects things when the repo lock
    # does not work.
    _metadbname = '0meta'
    _pathdbname = '1path'
    _nodedbname = '2node'
    _linkrevdbname = '3linkrev'

    def __init__(self, dirname):
        dbmname, self._dbm = _choosedbm()
        # use different file names for different dbm engine, to make the repo
        # rsync-friendly across different platforms.
        self._path = os.path.join(dirname, dbmname)
        self._dbs = {}

    def getlinkrevs(self, path, fnode):
        pathdb = self._getdb(self._pathdbname)
        nodedb = self._getdb(self._nodedbname)
        lrevdb = self._getdb(self._linkrevdbname)
        try:
            pathid = pathdb[path]
            nodeid = nodedb[fnode]
            v = lrevdb[pathid + '\0' + nodeid]
            return _str2intlist(v)
        except KeyError:
            return []

    def getlastrev(self):
        return _str2int(self._getmeta('lastrev'))

    def close(self):
        # the check is necessary if __init__ fails - the caller may call
        # "close" in a "finally" block and it probably does not want close() to
        # raise an exception there.
        if util.safehasattr(self, '_dbs'):
            for db in self._dbs.itervalues():
                db.close()
            self._dbs.clear()

    def _getmeta(self, name):
        try:
            return self._getdb(self._metadbname)[name]
        except KeyError:
            return ''

    def _getdb(self, name):
        if name not in self._dbs:
            self._dbs[name] = self._dbm.open(self._path + name, self._openflag)
        return self._dbs[name]

class linkrevdbreadwrite(linkrevdbreadonly):
    _openflag = 'c'

    def __init__(self, dirname):
        util.makedirs(dirname)
        super(linkrevdbreadwrite, self).__init__(dirname)

    def appendlinkrev(self, path, fnode, linkrev):
        pathdb = self._getdb(self._pathdbname)
        nodedb = self._getdb(self._nodedbname)
        lrevdb = self._getdb(self._linkrevdbname)
        metadb = self._getdb(self._metadbname)
        try:
            pathid = pathdb[path]
        except KeyError:
            pathid = _strinc(self._getmeta('pathid'))
            pathdb[path] = pathid
            metadb['pathid'] = pathid
        try:
            nodeid = nodedb[fnode]
        except KeyError:
            nodeid = _strinc(self._getmeta('nodeid'))
            nodedb[fnode] = nodeid
            metadb['nodeid'] = nodeid
        k = pathid + '\0' + nodeid
        try:
            v = _str2intlist(lrevdb[k])
        except KeyError:
            v = []
        if linkrev in v:
            return
        v.append(linkrev)
        lrevdb[k] = _intlist2str(v)

    def setlastrev(self, rev):
        self._getdb(self._metadbname)['lastrev'] = _int2str(rev)

class linkrevdbwritewithtemprename(linkrevdbreadwrite):
    # Some dbm (ex. gdbm) disallows writer and reader to co-exist. This is
    # basically to workaround that so a writer can still write to the (copied)
    # database when there is a reader.
    # Unlike "atomictemp", this applies to a directory. A directory cannot
    # work like "atomictemp" unless symlink is used. Symlink is not portable so
    # we don't use them. Therefore this is not atomic (while probably good
    # enough because we write files in a reasonable order - in the worst case,
    # we just drop those cache files).
    # Ideally, we can have other dbms which support reader and writer to
    # co-exist, and this will become unnecessary.
    def __init__(self, dirname):
        self._origpath = dirname
        head, tail = os.path.split(dirname)
        tempdir = '%s-%s' % (dirname, os.getpid())
        self._tempdir = tempdir
        try:
            shutil.copytree(dirname, tempdir)
            super(linkrevdbwritewithtemprename, self).__init__(tempdir)
        except Exception:
            shutil.rmtree(tempdir)
            raise

    def close(self):
        super(linkrevdbwritewithtemprename, self).close()
        if util.safehasattr(self, '_tempdir'):
            for name in sorted(os.listdir(self._tempdir)):
                oldpath = os.path.join(self._tempdir, name)
                newpath = os.path.join(self._origpath, name)
                os.rename(oldpath, newpath)
            os.rmdir(self._tempdir)

def linkrevdb(dirname, write=False, copyonwrite=False):
    # As commented in the "linkrevdbwritewithtemprename" above, these flags
    # (write, copyonwrite) are mainly designed to workaround gdbm's locking
    # issues. If we have a dbm that uses a less aggressive lock, we could get
    # rid of these workarounds.
    if not write:
        return linkrevdbreadonly(dirname)
    else:
        if copyonwrite:
            return linkrevdbwritewithtemprename(dirname)
        else:
            return linkrevdbreadwrite(dirname)

_linkrevdbpath = 'cache/linkrevdb'

def reposetup(ui, repo):
    if repo.local():
        # if the repo is single headed, adjustlinkrev can just return linkrev
        repo._singleheaded = (len(repo.unfiltered().changelog.headrevs()) == 1)

        dbpath = repo.vfs.join(_linkrevdbpath)
        setattr(repo, '_linkrevcache', linkrevdb(dbpath, write=False))
