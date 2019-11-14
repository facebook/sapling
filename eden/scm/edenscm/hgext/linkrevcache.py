# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# linkrevcache: a simple caching layer to speed up _adjustlinkrev

"""a simple caching layer to speed up _adjustlinkrev

The linkrevcache extension could use a pre-built database to speed up some
_adjustlinkrev operations. The database is stored in the directory
'.hg/cache/linkrevdb'.

To use the extension, you need to prebuild the database using the
`debugbuildlinkrevcache` command, and then keep the extension enabled.

To update the database, run `debugbuildlinkrevcache` again. It would find new
revisions and fill the database incrementally.

If the building process is slow, try setting `checkancestor` to False.

The database won't be updated on demand for I/O and locking concerns. It may be
addressed if we could have some (partially) "append-only" map-like data
structure.

The linkrev caching database would generally speed up the log (following a
file) and annotate operations.

.. note::

   The database format is not guaranteed portable. Copying it from a machine
   to another may make it unreadable.

Config examples::

    [linkrevcache]
    # Whether to test ancestors or not. (default: True)
    # - When set to False, the build process will be faster, while the database
    #   will contain some unnecessary entries (mode-only changes and merges
    #   where the file node is reused).
    # - When set to True, the database won't contain unnecessary entries.
    checkancestor = False

    # Whether to read filelog or not. (default: True)
    # - When set to False, the build process will be faster, while the database
    #   will be probably much larger.
    # - When set to True, filelog will be read and existing linkrevs won't be
    #   stored in the database.
    readfilelog = False

    # Upper bound fo memory usage for debugbuildlinkrevcache (default: 2441406)
    # - debugbuildlinkrevcache will try to reduce memory to sastify the limit
    # - has no effect if readfilelog is False
    # - has no effect for non-Linux platforms
    # - it is a best effort and the program might fail to sastify the limit
    maxpagesize = 2441406
"""

import os
import shutil
import sys

from edenscm.mercurial import (
    context,
    extensions,
    filelog,
    node,
    progress,
    pycompat,
    registrar,
    util,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import range


testedwith = "ships-with-fb-hgext"

cmdtable = {}
command = registrar.command(cmdtable)

_chosendbm = None


def _choosedbm():
    """return (name, module)"""
    global _chosendbm
    if not _chosendbm:
        if sys.version_info >= (3, 0):
            candidates = [
                ("gdbm", "dbm.gnu"),
                ("ndbm", "dbm.ndbm"),
                ("dumb", "dbm.dumb"),
            ]
        else:
            candidates = [
                ("gdbm", "gdbm"),
                ("bsd", "dbhash"),
                ("ndbm", "dbm"),
                ("dumb", "dumbdbm"),
            ]
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
        return "\x01"
    n = ord(s[-1])
    if n == 255:
        return _strinc(s[:-1]) + "\x01"
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
    s = ""
    while x:
        s = chr(x & 255) + s
        x >>= 8
    return s


def _intlist2str(intlist):
    result = ""
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
        result.append(_str2int(s[i : i + l]))
        i += l
    return result


class linkrevdbreadonly(object):
    _openflag = "r"

    # numbers are useful in the atomic replace case: they can be sorted
    # and replaced in a safer order. however, atomic caller should always
    # use repo lock so the order only protects things when the repo lock
    # does not work.
    _metadbname = "0meta"
    _pathdbname = "1path"
    _nodedbname = "2node"
    _linkrevdbname = "3linkrev"

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
            v = lrevdb[pathid + "\0" + nodeid]
            return _str2intlist(v)
        except KeyError:
            return []

    def getlastrev(self):
        return _str2int(self._getmeta("lastrev"))

    def close(self):
        # the check is necessary if __init__ fails - the caller may call
        # "close" in a "finally" block and it probably does not want close() to
        # raise an exception there.
        if util.safehasattr(self, "_dbs"):
            for db in self._dbs.itervalues():
                db.close()
            self._dbs.clear()

    def _getmeta(self, name):
        try:
            return self._getdb(self._metadbname)[name]
        except KeyError:
            return ""

    def _getdb(self, name):
        if name not in self._dbs:
            self._dbs[name] = self._dbm.open(self._path + name, self._openflag)
        return self._dbs[name]


class linkrevdbreadwrite(linkrevdbreadonly):
    _openflag = "c"

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
            pathid = _strinc(self._getmeta("pathid"))
            pathdb[path] = pathid
            metadb["pathid"] = pathid
        try:
            nodeid = nodedb[fnode]
        except KeyError:
            nodeid = _strinc(self._getmeta("nodeid"))
            nodedb[fnode] = nodeid
            metadb["nodeid"] = nodeid
        k = pathid + "\0" + nodeid
        try:
            v = _str2intlist(lrevdb[k])
        except KeyError:
            v = []
        if linkrev in v:
            return
        v.append(linkrev)
        lrevdb[k] = _intlist2str(v)

    def setlastrev(self, rev):
        self._getdb(self._metadbname)["lastrev"] = _int2str(rev)


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
        tempdir = "%s-%s" % (dirname, os.getpid())
        self._tempdir = tempdir
        try:
            shutil.copytree(dirname, tempdir)
            super(linkrevdbwritewithtemprename, self).__init__(tempdir)
        except Exception:
            shutil.rmtree(tempdir)
            raise

    def close(self):
        super(linkrevdbwritewithtemprename, self).close()
        if util.safehasattr(self, "_tempdir"):
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


_linkrevdbpath = "cache/linkrevdb"


def reposetup(ui, repo):
    if repo.local():
        dbpath = repo.localvfs.join(_linkrevdbpath)
        setattr(repo, "_linkrevcache", linkrevdb(dbpath, write=False))


@command(
    "debugbuildlinkrevcache",
    [
        ("e", "end", "", _("end revision")),
        (
            "",
            "copy",
            False,
            _("copy the database files to modify them " "lock-free (EXPERIMENTAL)"),
        ),
    ],
)
def debugbuildlinkrevcache(ui, repo, *pats, **opts):
    """build the linkrev database from filelogs"""
    db = linkrevdb(
        repo.localvfs.join(_linkrevdbpath),
        write=True,
        copyonwrite=opts.get("atomic_temp"),
    )
    end = int(opts.get("end") or (len(repo) - 1))
    try:
        _buildlinkrevcache(ui, repo, db, end)
    finally:
        db.close()


def _getrsspagecount():
    """Get RSS memory usage in pages. Only works on Linux"""
    try:
        # The second column is VmRSS. See "man procfs".
        return sum(map(int, open("/proc/self/statm").read().split()[1]))
    except Exception:
        return 0


def _buildlinkrevcache(ui, repo, db, end):
    checkancestor = ui.configbool("linkrevcache", "checkancestor", True)
    readfilelog = ui.configbool("linkrevcache", "readfilelog", True)
    # 2441406: 10G by default (assuming page size = 4K).
    maxpagesize = ui.configint("linkrevcache", "maxpagesize") or 2441406

    repo = repo.unfiltered()
    cl = repo.changelog
    idx = cl.index
    ml = repo.manifestlog

    filelogcache = {}

    def _getfilelog(path):
        if path not in filelogcache:
            # Make memory usage bounded
            if len(filelogcache) % 1000 == 0:
                if _getrsspagecount() > maxpagesize:
                    filelogcache.clear()
            filelogcache[path] = filelog.filelog(repo.svfs, path)
        return filelogcache[path]

    start = db.getlastrev() + 1

    # the number of ancestor tests when the slow (Python) stateful (cache
    # ancestors) algorithm is faster than the fast (C) stateless (walk through
    # the changelog index every time) algorithm.
    ancestorcountthreshold = 10

    with progress.bar(ui, _("building"), _("changesets"), end) as prog:
        for rev in range(start, end + 1):
            prog.value = rev
            clr = cl.changelogrevision(rev)
            md = ml[clr.manifest].read()

            if checkancestor:
                if len(clr.files) >= ancestorcountthreshold:
                    # we may need to frequently test ancestors against rev,
                    # in this case, pre-calculating rev's ancestors helps.
                    ancestors = cl.ancestors([rev])

                    def isancestor(x):
                        return x in ancestors

                else:
                    # the C index ancestor testing is faster than Python's
                    # lazyancestors.
                    def isancestor(x):
                        return x in idx.commonancestorsheads(x, rev)

            for path in clr.files:
                if path not in md:
                    continue

                fnode = md[path]

                if readfilelog:
                    fl = _getfilelog(path)
                    frev = fl.rev(fnode)
                    lrev = fl.linkrev(frev)
                    if lrev == rev:
                        continue
                else:
                    lrev = None

                if checkancestor:
                    linkrevs = set(db.getlinkrevs(path, fnode))
                    if lrev is not None:
                        linkrevs.add(lrev)
                    if rev in linkrevs:
                        continue
                    if any(isancestor(l) for l in linkrevs):
                        continue

                # found a new linkrev!
                if ui.debugflag:
                    ui.debug("%s@%s: new linkrev %s\n" % (path, node.hex(fnode), rev))

                db.appendlinkrev(path, fnode, rev)

            db.setlastrev(rev)


@command("debugverifylinkrevcache", [])
def debugverifylinkrevcache(ui, repo, *pats, **opts):
    """read the linkrevs from the database and verify if they are correct"""
    # restore to the original _adjustlinkrev implementation
    c = context.basefilectx
    extensions.unwrapfunction(c, "_adjustlinkrev", _adjustlinkrev)

    paths = {}  # {id: name}
    nodes = {}  # {id: name}

    repo = repo.unfiltered()
    idx = repo.unfiltered().changelog.index

    db = repo._linkrevcache
    paths = dict(db._getdb(db._pathdbname))
    nodes = dict(db._getdb(db._nodedbname))
    pathsrev = dict((v, k) for k, v in paths.iteritems())
    nodesrev = dict((v, k) for k, v in nodes.iteritems())
    lrevs = dict(db._getdb(db._linkrevdbname))

    readfilelog = ui.configbool("linkrevcache", "readfilelog", True)

    total = len(lrevs)
    with progress.bar(ui, _("verifying"), total=total) as prog:
        for i, (k, v) in enumerate(lrevs.iteritems()):
            prog.value = i
            pathid, nodeid = k.split("\0")
            path = pathsrev[pathid]
            fnode = nodesrev[nodeid]
            linkrevs = _str2intlist(v)
            linkrevs.sort()

            for linkrev in linkrevs:
                fctx = repo[linkrev][path]
                introrev = fctx.introrev()
                fctx.linkrev()
                if readfilelog:
                    flinkrev = fctx.linkrev()
                else:
                    flinkrev = None
                if introrev == linkrev:
                    continue
                if introrev in idx.commonancestorsheads(introrev, linkrev) and (
                    introrev in linkrevs or introrev == flinkrev
                ):
                    adjective = _("unnecessary")
                else:
                    adjective = _("incorrect")
                ui.warn(
                    _("%s linkrev %s for %s @ %s (expected: %s)\n")
                    % (adjective, linkrev, path, node.hex(fnode), introrev)
                )

    ui.write(_("%d entries verified\n") % total)


def _adjustlinkrev(orig, self, *args, **kwds):
    lkr = self.linkrev()
    repo = self._repo

    # argv can be "path, flog, fnode, srcrev", or "srcrev" - see e81d72b4b0ae
    srcrev = args[-1]
    cache = getattr(self._repo, "_linkrevcache", None)
    if cache is not None and srcrev is not None:
        index = repo.unfiltered().changelog.index
        try:
            linkrevs = set(cache.getlinkrevs(self._path, self._filenode))
        except Exception:
            # the database may be locked - cannot be used correctly
            linkrevs = set()
        finally:
            # do not keep the database open so others can write to it
            # note: this is bad for perf. but it's here to workaround the gdbm
            # locking pattern: reader and writer cannot co-exist. if we have
            # a dbm engine that locks differently, we don't need this.
            cache.close()
        linkrevs.add(lkr)
        for rev in sorted(linkrevs):  # sorted filters out unnecessary linkrevs
            if rev in index.commonancestorsheads(rev, srcrev):
                return rev

    # fallback to the possibly slow implementation
    return orig(self, *args, **kwds)


def uisetup(ui):
    c = context.basefilectx
    extensions.wrapfunction(c, "_adjustlinkrev", _adjustlinkrev)
