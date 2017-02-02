'''extension that makes node prefix lookup faster

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
'''

from collections import defaultdict

from mercurial import (
    cmdutil,
    context,
    error,
    extensions,
    revlog,
    scmutil,
)

from mercurial.i18n import _
from mercurial.node import (
    bin,
    hex,
)

from operator import itemgetter

import os
import re
import struct

LookupError = error.LookupError

cmdtable = {}
command = cmdutil.command(cmdtable)

_partialindexdir = 'partialindex'

_maybehash = re.compile(r'^[a-f0-9]+$').search
_packstruct = struct.Struct('!L')

_nodesize = 20
_entrysize = _nodesize + _packstruct.size
_raiseifinconsistent = False
_current_version = 1
_tip = 'run `hg debugrebuildpartialindex` to fix the issue'

def extsetup(ui):
    extensions.wrapfunction(context.changectx, '__init__', _changectxinit)
    extensions.wrapfunction(revlog.revlog, '_partialmatch', _partialmatch)
    global _raiseifinconsistent
    _raiseifinconsistent = ui.configbool('fastpartialmatch',
                                         'raiseifinconsistent', False)

def reposetup(ui, repo):
    if repo.local():
        # Add `ui` object to access it from `_partialmatch` func
        repo.svfs.ui = ui
        ui.setconfig('hooks', 'pretxncommit.fastpartialmatch', _commithook)
        ui.setconfig('hooks', 'pretxnchangegroup.fastpartialmatch',
                     _changegrouphook)
        # To handle strips
        ui.setconfig('hooks', 'pretxnclose.fastpartialmatch', _pretxnclosehook)
        # Increase the priority of the hook to make sure it's called before
        # other hooks. If another hook failed before
        # pretxnclose.fastpartialmatch during strip then partial index will
        # contain non-existing nodes.
        ui.setconfig('hooks', 'priority.pretxnclose.fastpartialmatch',
                     10)

@command('^debugprintpartialindexfile', [])
def debugprintpartialindexfile(ui, repo, *args):
    '''Parses and prints partial index files
    '''
    if not args:
        raise error.Abort(_('please specify a filename'))

    for file in args:
        fullpath = os.path.join(_partialindexdir, file)
        if not repo.svfs.exists(fullpath):
            ui.warn(_('file %s does not exist\n') % file)
            continue

        for node, rev in _parseindexfile(repo.svfs, fullpath):
            ui.write('%s %d\n' % (hex(node), rev))

@command('^debugrebuildpartialindex', [])
def debugrebuildpartialindex(ui, repo):
    '''Rebuild partial index from scratch
    '''
    _rebuildpartialindex(ui, repo)

@command('^debugcheckpartialindex', [])
def debugcheckfastpartialindex(ui, repo):
    '''Command to check that partial index is consistent

    It checks that revision numbers are correct and checks that partial index
    has all the nodes from the repo.
    '''
    indexvfs = scmutil.vfs(repo.svfs.join(_partialindexdir))
    foundnodes = set()
    # Use unfiltered repo because index may have entries that point to hidden
    # commits
    ret = 0
    repo = repo.unfiltered()
    for entry in indexvfs.listdir():
        if len(entry) == 2 and indexvfs.isfile(entry):
            try:
                for node, actualrev in _parseindexfile(indexvfs, entry):
                    expectedrev = repo.changelog.rev(node)
                    foundnodes.add(node)
                    if expectedrev != actualrev:
                        ret = 1
                        ui.warn(_('corrupted index: rev number for %s ' +
                                'should be %d but found %d\n') %
                                (hex(node), expectedrev, actualrev))
            except ValueError as e:
                ret = 1
                ui.warn(_('%s file is corrupted: %s\n') % (entry, e))

    for rev in repo:
        node = repo[rev].node()
        if node not in foundnodes:
            ret = 1
            ui.warn(_('%s node not found in partialindex\n') % hex(node))
    return ret

@command('^debugresolvepartialhash', [])
def debugresolvepartialhash(ui, repo, *args):
    for arg in args:
        ui.debug('resolving %s' % arg)
        candidates = _findcandidates(ui, repo.svfs, arg)
        if candidates is None:
            ui.write(_('failed to read partial index\n'))
        elif len(candidates) == 0:
            ui.write(_('%s not found') % arg)
        else:
            nodes = ', '.join([hex(node) + ' ' + str(rev)
                               for node, rev in candidates.items()])
            ui.write(_('%s: %s\n') % (arg, nodes))

def _rebuildpartialindex(ui, repo, skiphexnodes=None):
    ui.debug('rebuilding partial node index\n')
    repo = repo.unfiltered()
    if not skiphexnodes:
        skiphexnodes = set()
    vfs = repo.svfs
    tempdir = '.tmp' + _partialindexdir

    if vfs.exists(_partialindexdir):
        vfs.rmtree(_partialindexdir)
    if vfs.exists(tempdir):
        vfs.rmtree(tempdir)

    vfs.mkdir(tempdir)

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
        with indexvfs(filename, 'a') as fileobj:
            header = _header(len(data))
            header.write(fileobj)
            for node, rev in sorted(data, key=itemgetter(0)):
                _writeindexentry(fileobj, node, rev)
    vfs.rename(tempdir, _partialindexdir)

def _getopener(path):
    vfs = scmutil.vfs(path)
    vfs.createmode = 0o644
    return vfs

def _pretxnclosehook(ui, repo, hooktype, txnname, **hookargs):
    # Strip may change revision numbers for many commits, it's safer to rebuild
    # index from scratch.
    if txnname == 'strip':
        vfs = repo.svfs
        if vfs.exists(_partialindexdir):
            vfs.rmtree(_partialindexdir)
            _rebuildpartialindex(ui, repo)

def _commithook(ui, repo, hooktype, node, parent1, parent2):
    if _ispartialindexbuilt(repo.svfs):
        # Append new entries only if index is built
        hexnode = node  # it's actually a hexnode
        tr = repo.currenttransaction()
        _recordcommit(tr, hexnode, repo[hexnode].rev(), repo.svfs)

def _changegrouphook(ui, repo, hooktype, **hookargs):
    tr = repo.currenttransaction()
    vfs = repo.svfs
    if 'node' in hookargs and 'node_last' in hookargs:
        hexnode_first = hookargs['node']
        hexnode_last = hookargs['node_last']
        # Ask changelog directly to avoid calling fastpartialmatch because
        # it doesn't have the newest nodes yet
        rev_first = repo.changelog.rev(bin(hexnode_first))
        rev_last = repo.changelog.rev(bin(hexnode_last))
        newhexnodes = []
        for rev in xrange(rev_first, rev_last + 1):
            newhexnodes.append(repo[rev].hex())

        if not vfs.exists(_partialindexdir):
            _rebuildpartialindex(ui, repo, skiphexnodes=set(newhexnodes))
        for i, hexnode in enumerate(newhexnodes):
            _recordcommit(tr, hexnode, rev_first + i, vfs)
    else:
        ui.warn(_('unexpected hookargs parameters: `node` and ' +
                  '`node_last` should be present\n'))

def _recordcommit(tr, hexnode, rev, vfs):
    vfs = _getopener(vfs.join(''))
    filename = os.path.join(_partialindexdir, hexnode[:2])
    if vfs.exists(filename):
        size = vfs.stat(filename).st_size
    else:
        size = 0
    tr.add(filename, size)
    with vfs(filename, 'a') as fileobj:
        if not size:
            header = _header(0)
            header.write(fileobj)
        _writeindexentry(fileobj, bin(hexnode), rev)

def _partialmatch(orig, self, id):
    if not _ispartialindexbuilt(self.opener):
        return orig(self, id)
    candidates = _findcandidates(self.opener.ui, self.opener, id)
    if candidates is None:
        return orig(self, id)
    elif len(candidates) == 0:
        origres = orig(self, id)
        if origres is not None:
            return _handleinconsistentindex(id, origres)
        return None
    elif len(candidates) == 1:
        return candidates.keys()[0]
    else:
        raise LookupError(id, _partialindexdir, _('ambiguous identifier'))

def _changectxinit(orig, self, repo, changeid=''):
    if not _ispartialindexbuilt(repo.svfs):
        return orig(self, repo, changeid)
    candidates = _findcandidates(repo.ui, repo.svfs, changeid)
    if candidates is None:
        return orig(self, repo, changeid)
    elif len(candidates) == 0:
        origres = orig(self, repo, changeid)
        if origres is not None:
            return _handleinconsistentindex(changeid, origres)
        return None
    elif len(candidates) == 1:
        rev = candidates.values()[0]
        repo.ui.debug('using partial index cache %d\n' % rev)
        return orig(self, repo, rev)
    else:
        raise LookupError(id, _partialindexdir, _('ambiguous identifier'))

def _handleinconsistentindex(changeid, expected):
    if _raiseifinconsistent:
        raise ValueError('inconsistent partial match index while resolving %s' %
                         changeid)
    else:
        return expected

def _ispartialindexbuilt(vfs):
    return vfs.exists(_partialindexdir)

def _findcandidates(ui, vfs, id):
    '''Returns dict with matching candidates or None if error happened
    '''
    candidates = {}
    if not (isinstance(id, str) and len(id) >= 4 and _maybehash(id)):
        return candidates
    filename = id[:2]
    fullpath = os.path.join(_partialindexdir, filename)
    try:
        if vfs.exists(fullpath):
            for node, rev in _parseindexfile(vfs, fullpath):
                hexnode = hex(node)
                if hexnode.startswith(id):
                    candidates[node] = rev
    except Exception as e:
        ui.warn(_('failed to read partial index %s : %s\n') %
                (fullpath, str(e)))
        return None
    return candidates

class _header(object):
    _versionpack = struct.Struct('!B')
    _intpacker = _packstruct
    headersize = 24

    def __init__(self, sortedcount):
        self.sortedcount = sortedcount

    def write(self, fileobj):
        fileobj.write(self._versionpack.pack(_current_version))
        fileobj.write(self._intpacker.pack(self.sortedcount))
        fill = '\0' * (self.headersize - self._intpacker.size -
                       self._versionpack.size)
        fileobj.write(fill)

    @classmethod
    def read(cls, fileobj):
        header = fileobj.read(cls.headersize)
        if not header or len(header) != cls.headersize:
            raise ValueError(_('corrupted header: %s') % _tip)

        versionsize = cls._versionpack.size
        headerversion = header[:versionsize]
        headerversion = cls._versionpack.unpack(headerversion)[0]
        if headerversion != _current_version:
            raise ValueError(_('incompatible index version: %s') % _tip)

        sortedcount = header[versionsize:versionsize + cls._intpacker.size]
        sortedcount = cls._intpacker.unpack(sortedcount)[0]
        return cls(sortedcount)

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

def _readindexentry(fileobj):
    line = fileobj.read(_entrysize)
    if not line:
        return None, None
    if len(line) != _entrysize:
        raise ValueError(_('corrupted index: %s') % _tip)

    node = line[:_nodesize]
    rev = line[_nodesize:]
    rev = _packstruct.unpack(rev)
    return node, rev[0]
