'''extension that makes node prefix lookup faster

Storage format is simple. There are a few files (at most 256).
Each file contains entries:
  <20-byte node hash><4 byte encoded rev>
Name of the file is the first two letters of the hex node hash. Nodes with the
same first two letters go to the same file. Nodes are NOT sorted inside the file
to make appends of new nodes easier.
Partial index should always be correct i.e. it should contain only nodes that
are present in the repo (regardless of whether they are visible or not) and
rev numbers for nodes should be correct too.
'''

from mercurial import (
    cmdutil,
    error,
    scmutil,
)

from mercurial.i18n import _
from mercurial.node import (
    bin,
    hex,
)

import os
import struct

LookupError = error.LookupError

cmdtable = {}
command = cmdutil.command(cmdtable)

_partialindexdir = 'partialindex'

_packstruct = struct.Struct('!L')

_nodesize = 20
_entrysize = _nodesize + _packstruct.size

def reposetup(ui, repo):
    if repo.local():
        ui.setconfig('hooks', 'pretxncommit.fastpartialmatch', _commithook)

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

        for node, rev in _readallentries(repo.svfs, fullpath):
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
                for node, actualrev in _readallentries(indexvfs, entry):
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

    # Cache open file objects to not reopen the same files many times
    fileobjs = {}
    try:
        indexvfs = _getopener(vfs.join(tempdir))
        for rev in repo.changelog:
            node = repo.changelog.node(rev)
            hexnode = hex(node)
            if hexnode in skiphexnodes:
                continue
            filename = hexnode[:2]
            if filename not in fileobjs:
                fileobjs[filename] = indexvfs(filename, mode='a')
            fileobj = fileobjs[filename]
            _writeindexentry(fileobj, node, rev)
        vfs.rename(tempdir, _partialindexdir)
    finally:
        for fileobj in fileobjs.values():
            fileobj.close()

def _getopener(path):
    vfs = scmutil.vfs(path)
    vfs.createmode = 0o644
    return vfs

def _commithook(ui, repo, hooktype, node, parent1, parent2):
    if _ispartialindexbuilt(repo.svfs):
        # Append new entries only if index is built
        hexnode = node  # it's actually a hexnode
        tr = repo.currenttransaction()
        _recordcommit(tr, hexnode, repo[hexnode].rev(), repo.svfs)

def _recordcommit(tr, hexnode, rev, vfs):
    vfs = _getopener(vfs.join(''))
    filename = os.path.join(_partialindexdir, hexnode[:2])
    if vfs.exists(filename):
        size = vfs.stat(filename).st_size
    else:
        size = 0
    tr.add(filename, size)
    with vfs(filename, 'a') as fileobj:
        _writeindexentry(fileobj, bin(hexnode), rev)

def _ispartialindexbuilt(vfs):
    return vfs.exists(_partialindexdir)

def _writeindexentry(fileobj, node, rev):
    fileobj.write(node + _packstruct.pack(rev))

def _readallentries(vfs, file):
    with vfs(file) as fileobj:
        node, rev = _readindexentry(fileobj)
        while node:
            yield node, rev
            node, rev = _readindexentry(fileobj)

def _readindexentry(fileobj):
    line = fileobj.read(_entrysize)
    if not line:
        return None, None
    if len(line) != _entrysize:
        raise ValueError(_('corrupted index'))

    node = line[:_nodesize]
    rev = line[_nodesize:]
    rev = _packstruct.unpack(rev)
    return node, rev[0]
