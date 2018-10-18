"""extension that makes node prefix lookup faster

Storage format is simple. There are a few files (at most 256).
Each file contains header and entries:
Header:

  <1 byte version><4 bytes number of entries that were sorted><19 bytes unused>

Entry:

  <20-byte node hash><4 byte encoded rev>

Name of the file is the first two letters of the hex node hash. Nodes with the
same first two letters go to the same file. Nodes may be partially sorted:
first entries are sorted others don't. Header stores info about how many entries
are sorted.
Partial index should always be correct i.e. it should contain only nodes that
are present in the repo (regardless of whether they are visible or not) and
rev numbers for nodes should be correct too.

::

    [fastpartialmatch]
    # if option is set then exception is raised if index result is inconsistent
    # with slow path
    raiseifinconsistent = False

    # whether to use bisect during partial hash resolving
    usebisect = True

    # if any index file has more than or equal to `unsortedthreshold` unsorted
    # entries then index will be rebuilt when _changegrouphook will be triggered
    # (usually it's the next pull)
    unsortedthreshold = 1000

    # if fastpartialmatch extension was temporarily disabled then index may miss
    # some entries. By bumping generationnumber we can force index to be rebuilt
    generationnumber = 0

    # internal config setting. Marks index as needed to be rebuilt
    rebuild = False
"""

import os
import re
import struct
from collections import defaultdict
from functools import partial
from operator import itemgetter

from mercurial import (
    changelog,
    dispatch,
    error,
    extensions,
    localrepo,
    registrar,
    util,
    vfs as vfsmod,
)
from mercurial.i18n import _
from mercurial.node import bin, hex, nullhex, nullid, nullrev

from .generic_bisect import bisect


LookupError = error.LookupError

cmdtable = {}
command = registrar.command(cmdtable)

_partialindexdir = "partialindex"

_maybehash = re.compile(r"^[a-f0-9]+$").search
_packstruct = struct.Struct("!L")

_nodesize = 20
_entrysize = _nodesize + _packstruct.size
_raiseifinconsistent = False
_usebisect = True
_current_version = 1
_tip = "run `hg debugrebuildpartialindex` to fix the issue"
_unsortedthreshold = 1000
_needrebuildfile = "partialindexneedrebuild"

try:
    xrange(0)
except NameError:
    xrange = range


def extsetup(ui):
    # developer config: extensions.clindex
    if ui.config("extensions", "clindex") == "":
        # do nothing, if clindex is enabled
        return
    extensions.wrapfunction(changelog.changelog, "_partialmatch", _partialmatch)
    extensions.wrapfunction(localrepo.localrepository, "commit", _localrepocommit)
    extensions.wrapfunction(
        localrepo.localrepository, "transaction", _localrepotransaction
    )
    global _raiseifinconsistent
    _raiseifinconsistent = ui.configbool(
        "fastpartialmatch", "raiseifinconsistent", False
    )
    global _usebisect
    _usebisect = ui.configbool("fastpartialmatch", "usebisect", True)
    global _unsortedthreshold
    _unsortedthreshold = ui.configint(
        "fastpartialmatch", "unsortedthreshold", _unsortedthreshold
    )

    def _runcommand(orig, lui, repo, cmd, fullargs, ui, *args, **kwargs):
        res = orig(lui, repo, cmd, fullargs, ui, *args, **kwargs)
        if ui.config("fastpartialmatch", "rebuild", False):
            _markneedsrebuilding(ui, repo)
        return res

    extensions.wrapfunction(dispatch, "runcommand", _runcommand)
    # Add _needrebuildfile to the list of files that don't need to be protected
    # by wlock. Race conditions on _needrebuildfile are not important because
    # at worst it may trigger rebuilding twice or postpone index rebuilding.
    localrepo.localrepository._wlockfreeprefix.add(_needrebuildfile)


def reposetup(ui, repo):
    if ui.config("extensions", "clindex") == "":
        # do nothing, if clindex is enabled
        return
    isbundlerepo = repo.url().startswith("bundle:")
    if repo.local() and not isbundlerepo:
        # Add `ui` object and `usefastpartialmatch` to access it
        # from `_partialmatch` func
        repo.svfs.ui = ui
        repo.svfs.usefastpartialmatch = True
        ui.setconfig("hooks", "pretxncommit.fastpartialmatch", _commithook)
        ui.setconfig("hooks", "pretxnchangegroup.fastpartialmatch", _changegrouphook)
        # To handle strips
        ui.setconfig("hooks", "pretxnclose.fastpartialmatch", _pretxnclosehook)
        # Increase the priority of the hook to make sure it's called before
        # other hooks. If another hook failed before
        # pretxnclose.fastpartialmatch during strip then partial index will
        # contain non-existing nodes.
        ui.setconfig("hooks", "priority.pretxnclose.fastpartialmatch", 10)

        if _ispartialindexbuilt(repo.svfs):
            actualgennum = _readgenerationnum(ui, repo.svfs)
            expectedgennum = ui.configint("fastpartialmatch", "generationnumber", 0)
            if actualgennum != expectedgennum:
                repo.svfs.rmtree(_partialindexdir)


@command("^debugprintpartialindexfile", [])
def debugprintpartialindexfile(ui, repo, *args):
    """Parses and prints partial index files
    """
    if not args:
        raise error.Abort(_("please specify a filename"))

    for file in args:
        fullpath = os.path.join(_partialindexdir, file)
        if not repo.svfs.exists(fullpath):
            ui.warn(_("file %s does not exist\n") % file)
            continue

        for node, rev in _parseindexfile(repo.svfs, fullpath):
            ui.write("%s %d\n" % (hex(node), rev))


@command("^debugrebuildpartialindex", [])
def debugrebuildpartialindex(ui, repo):
    """Rebuild partial index from scratch
    """
    _rebuildpartialindex(ui, repo)


@command("^debugcheckpartialindex", [])
def debugcheckfastpartialindex(ui, repo):
    """Command to check that partial index is consistent

    It checks that revision numbers are correct and checks that partial index
    has all the nodes from the repo.
    """

    if not repo.svfs.exists(_partialindexdir):
        ui.warn(_("partial index is not built\n"))
        return 1
    indexvfs = vfsmod.vfs(repo.svfs.join(_partialindexdir))
    foundnodes = set()
    # Use unfiltered repo because index may have entries that point to hidden
    # commits
    ret = 0
    repo = repo.unfiltered()
    for indexfile in _iterindexfile(indexvfs):
        try:
            for node, actualrev in _parseindexfile(indexvfs, indexfile):
                expectedrev = repo.changelog.rev(node)
                foundnodes.add(node)
                if expectedrev != actualrev:
                    ret = 1
                    ui.warn(
                        _(
                            "corrupted index: rev number for %s "
                            + "should be %d but found %d\n"
                        )
                        % (hex(node), expectedrev, actualrev)
                    )
        except ValueError as e:
            ret = 1
            ui.warn(_("%s file is corrupted: %s\n") % (indexfile, e))

    for rev in repo:
        node = repo[rev].node()
        if node not in foundnodes:
            ret = 1
            ui.warn(_("%s node not found in partialindex\n") % hex(node))
    return ret


@command("^debugresolvepartialhash", [])
def debugresolvepartialhash(ui, repo, *args):
    for arg in args:
        ui.debug("resolving %s" % arg)
        candidates = _findcandidates(ui, repo.svfs, arg)
        if candidates is None:
            ui.write(_("failed to read partial index\n"))
        elif len(candidates) == 0:
            ui.write(_("%s not found") % arg)
        else:
            nodes = ", ".join(
                [hex(node) + " " + str(rev) for node, rev in candidates.items()]
            )
            ui.write(_("%s: %s\n") % (arg, nodes))


@command("^debugfastpartialmatchstat", [])
def debugfastpartialmatchstat(ui, repo):
    if not repo.svfs.exists(_partialindexdir):
        ui.warn(_("partial index is not built\n"))
        return 1
    generationnum = _readgenerationnum(ui, repo.svfs)
    ui.write(_("generation number: %d\n") % generationnum)
    if _needsrebuilding(repo):
        ui.write(_("index will be rebuilt on the next pull\n"))
    indexvfs = vfsmod.vfs(repo.svfs.join(_partialindexdir))
    for indexfile in sorted(_iterindexfile(indexvfs)):
        size = indexvfs.stat(indexfile).st_size - _header.headersize
        entriescount = size / _entrysize
        with indexvfs(indexfile) as fileobj:
            header = _header.read(fileobj)
            ui.write(
                _("file: %s, entries: %d, out of them %d sorted\n")
                % (indexfile, entriescount, header.sortedcount)
            )


def _localrepocommit(orig, self, *args, **kwargs):
    """Wrapper for localrepo.commit to record temporary amend commits

    Upstream mercurial disables all hooks for temporary amend commits.
    Use this hacky wrapper to record this commit anyway
    """

    node = orig(self, *args, **kwargs)
    if node is None:
        return node
    hexnode = hex(node)
    tr = self.currenttransaction()
    indexbuilt = _ispartialindexbuilt(self.svfs)
    if tr and hexnode not in tr.addedcommits and indexbuilt:
        _recordcommit(self.ui, tr, hexnode, self.changelog.rev(node), self.svfs)
    return node


def _localrepotransaction(orig, *args, **kwargs):
    tr = orig(*args, **kwargs)
    if not util.safehasattr(tr, "addedcommits"):
        tr.addedcommits = set()
    return tr


def _iterindexfile(indexvfs):
    for entry in indexvfs.listdir():
        if len(entry) == 2 and indexvfs.isfile(entry):
            yield entry


def _rebuildpartialindex(ui, repo, skiphexnodes=None):
    ui.debug("rebuilding partial node index\n")
    repo = repo.unfiltered()
    if not skiphexnodes:
        skiphexnodes = set()
    vfs = repo.svfs
    tempdir = ".tmp" + _partialindexdir

    if vfs.exists(_partialindexdir):
        vfs.rmtree(_partialindexdir)
    if vfs.exists(tempdir):
        vfs.rmtree(tempdir)

    vfs.mkdir(tempdir)
    _unmarkneedsrebuilding(repo)

    filesdata = defaultdict(list)
    for rev in repo.changelog:
        node = repo.changelog.node(rev)
        hexnode = hex(node)
        if hexnode in skiphexnodes:
            continue
        filename = hexnode[:2]
        filesdata[filename].append((node, rev))

    indexvfs = _getopener(vfs.join(tempdir))
    for filename, data in filesdata.items():
        with indexvfs(filename, "a") as fileobj:
            header = _header(len(data))
            header.write(fileobj)
            for node, rev in sorted(data, key=itemgetter(0)):
                _writeindexentry(fileobj, node, rev)

    with indexvfs("generationnum", "w") as fp:
        generationnum = ui.configint("fastpartialmatch", "generationnumber", 0)
        fp.write(str(generationnum))
    vfs.rename(tempdir, _partialindexdir)


def _getopener(path):
    vfs = vfsmod.vfs(path)
    vfs.createmode = 0o644
    return vfs


def _pretxnclosehook(ui, repo, hooktype, txnname, **hookargs):
    # Strip may change revision numbers for many commits, it's safer to rebuild
    # index from scratch.
    if txnname == "strip":
        vfs = repo.svfs
        if vfs.exists(_partialindexdir):
            vfs.rmtree(_partialindexdir)
            _rebuildpartialindex(ui, repo)


def _commithook(ui, repo, hooktype, node, parent1, parent2):
    if _ispartialindexbuilt(repo.svfs):
        # Append new entries only if index is built
        hexnode = node  # it's actually a hexnode
        tr = repo.currenttransaction()
        _recordcommit(ui, tr, hexnode, repo[hexnode].rev(), repo.svfs)


def _changegrouphook(ui, repo, hooktype, **hookargs):
    tr = repo.currenttransaction()
    vfs = repo.svfs
    if "node" in hookargs and "node_last" in hookargs:
        hexnode_first = hookargs["node"]
        hexnode_last = hookargs["node_last"]
        # Ask changelog directly to avoid calling fastpartialmatch because
        # it doesn't have the newest nodes yet
        rev_first = repo.changelog.rev(bin(hexnode_first))
        rev_last = repo.changelog.rev(bin(hexnode_last))
        newhexnodes = []
        for rev in xrange(rev_first, rev_last + 1):
            newhexnodes.append(repo[rev].hex())

        if not vfs.exists(_partialindexdir) or _needsrebuilding(repo):
            _rebuildpartialindex(ui, repo, skiphexnodes=set(newhexnodes))
        for i, hexnode in enumerate(newhexnodes):
            _recordcommit(ui, tr, hexnode, rev_first + i, vfs)
    else:
        ui.warn(
            _(
                "unexpected hookargs parameters: `node` and "
                + "`node_last` should be present\n"
            )
        )


def _recordcommit(ui, tr, hexnode, rev, vfs):
    vfs = _getopener(vfs.join(""))
    filename = os.path.join(_partialindexdir, hexnode[:2])
    if vfs.exists(filename):
        size = vfs.stat(filename).st_size
    else:
        size = 0
    tr.add(filename, size)
    try:
        with vfs(filename, "a") as fileobj:
            if not size:
                header = _header(0)
                header.write(fileobj)
            _writeindexentry(fileobj, bin(hexnode), rev)
    except (OSError, IOError) as e:
        # failed to record commit, index maybe inconsistent
        # let's delete it
        msgfmt = (
            "failed to record commit in partial index: %s, "
            + "index will be rebuilt on next pull\n"
        )
        ui.warn(_(msgfmt) % e)
        try:
            vfs.rmtree(_partialindexdir)
        except (OSError, IOError) as e:
            fullpath = vfs.join(_partialindexdir)
            msgfmt = "failed to remove %s: %s, please remove it manually\n"
            ui.warn(_(msgfmt) % (fullpath, e))
    tr.addedcommits.add(hexnode)


def _partialmatch(orig, self, id):
    # we only need the vfs for exists checks, not writing
    # so if opener doesn't have `exists` method then we can't use
    # partial index
    opener = self._realopener
    try:
        indexbuilt = _ispartialindexbuilt(opener)
        ui = opener.ui
    except AttributeError:
        # not a proper vfs, no exists method or ui, so we can't proceed.
        indexbuilt = False
    if not indexbuilt or not getattr(opener, "usefastpartialmatch", None):
        return orig(self, id)
    candidates = _findcandidates(ui, opener, id)
    if candidates is None:
        return orig(self, id)
    elif len(candidates) == 0:
        origres = orig(self, id)
        if origres is not None:
            return _handleinconsistentindex(id, origres)
        return None
    elif len(candidates) == 1:
        node, rev = candidates.popitem()
        ui.debug("using partial index cache %d\n" % rev)
        return node
    else:
        raise LookupError(id, _partialindexdir, _("ambiguous identifier"))


def _handleinconsistentindex(changeid, expected):
    if _raiseifinconsistent:
        raise ValueError(
            "inconsistent partial match index while resolving %s" % changeid
        )
    else:
        return expected


def _ispartialindexbuilt(vfs):
    return vfs.exists(_partialindexdir)


def _bisectcmp(fileobj, index, value):
    fileobj.seek(_entryoffset(index))
    node, rev = _readindexentry(fileobj)
    if node is None:
        raise ValueError(_("corrupted index: %s") % _tip)
    hexnode = hex(node)
    if hexnode.startswith(value):
        return 0
    if hexnode < value:
        return -1
    else:
        return 1


def _findcandidates(ui, vfs, id):
    """Returns dict with matching candidates or None if error happened
    """
    candidates = {}
    if not (isinstance(id, str) and len(id) >= 4 and _maybehash(id)):
        return candidates
    if nullhex.startswith(id):
        candidates[nullid] = nullrev
    filename = id[:2]
    fullpath = os.path.join(_partialindexdir, filename)
    try:
        if vfs.exists(fullpath):
            with vfs(fullpath) as fileobj:
                sortedcount = _header.read(fileobj).sortedcount
                if _usebisect:
                    ui.debug("using bisect\n")
                    compare = partial(_bisectcmp, fileobj)
                    entryindex = bisect(0, sortedcount - 1, compare, id)

                    if entryindex is not None:
                        node, rev = _readindexentry(fileobj, _entryoffset(entryindex))
                        while node and hex(node).startswith(id):
                            candidates[node] = rev
                            node, rev = _readindexentry(fileobj)

                    # bisect has found candidates among sorted entries.
                    # But there maybe candidates among unsorted entries that
                    # go after. Move file current position after all sorted
                    # entries and then scan the file till the end.
                    fileobj.seek(_entryoffset(sortedcount))

                unsorted = 0
                for node, rev in _readtillend(fileobj):
                    hexnode = hex(node)
                    unsorted += 1
                    if hexnode.startswith(id):
                        candidates[node] = rev
                if unsorted >= _unsortedthreshold:
                    ui.setconfig("fastpartialmatch", "rebuild", True)
    except Exception as e:
        ui.warn(_("failed to read partial index %s : %s\n") % (fullpath, str(e)))
        return None
    return candidates


class _header(object):
    _versionpack = struct.Struct("!B")
    _intpacker = _packstruct
    headersize = 24

    def __init__(self, sortedcount):
        self.sortedcount = sortedcount

    def write(self, fileobj):
        fileobj.write(self._versionpack.pack(_current_version))
        fileobj.write(self._intpacker.pack(self.sortedcount))
        fill = "\0" * (self.headersize - self._intpacker.size - self._versionpack.size)
        fileobj.write(fill)

    @classmethod
    def read(cls, fileobj):
        header = fileobj.read(cls.headersize)
        if not header or len(header) != cls.headersize:
            raise ValueError(_("corrupted header: %s") % _tip)

        versionsize = cls._versionpack.size
        headerversion = header[:versionsize]
        headerversion = cls._versionpack.unpack(headerversion)[0]
        if headerversion != _current_version:
            raise ValueError(_("incompatible index version: %s") % _tip)

        sortedcount = header[versionsize : versionsize + cls._intpacker.size]
        sortedcount = cls._intpacker.unpack(sortedcount)[0]
        return cls(sortedcount)


def _needsrebuilding(repo):
    return repo.localvfs.exists(_needrebuildfile)


def _markneedsrebuilding(ui, repo):
    try:
        with repo.localvfs(_needrebuildfile, "w") as fileobj:
            fileobj.write("content")  # content doesn't matter
    except IOError as e:
        ui.warn(_("error happened while triggering rebuild: %s\n") % e)


def _unmarkneedsrebuilding(repo):
    repo.localvfs.tryunlink(_needrebuildfile)


def _readgenerationnum(ui, vfs):
    generationnumfile = os.path.join(_partialindexdir, "generationnum")
    if not vfs.exists(generationnumfile):
        return 0
    try:
        with vfs(generationnumfile) as f:
            return int(f.read())
    except Exception as e:
        ui.warn(_("error happened while reading generation num: %s\n") % e)
    return 0


def _writeindexentry(fileobj, node, rev):
    fileobj.write(node + _packstruct.pack(rev))


def _parseindexfile(vfs, file):
    if vfs.stat(file).st_size == 0:
        return
    with vfs(file) as fileobj:
        _header.read(fileobj)
        for node, rev in _readtillend(fileobj):
            yield node, rev


def _readtillend(fileobj):
    node, rev = _readindexentry(fileobj)
    while node:
        yield node, rev
        node, rev = _readindexentry(fileobj)


def _entryoffset(index):
    return _header.headersize + _entrysize * index


def _readindexentry(fileobj, readfrom=None):
    if readfrom is not None:
        fileobj.seek(readfrom)
    line = fileobj.read(_entrysize)
    if not line:
        return None, None
    if len(line) != _entrysize:
        raise ValueError(_("corrupted index: %s") % _tip)

    node = line[:_nodesize]
    rev = line[_nodesize:]
    rev = _packstruct.unpack(rev)
    return node, rev[0]
