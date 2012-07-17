# obsolete.py - obsolete markers handling
#
# Copyright 2012 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Obsolete markers handling

An obsolete marker maps an old changeset to a list of new
changesets. If the list of new changesets is empty, the old changeset
is said to be "killed". Otherwise, the old changeset is being
"replaced" by the new changesets.

Obsolete markers can be used to record and distribute changeset graph
transformations performed by history rewriting operations, and help
building new tools to reconciliate conflicting rewriting actions. To
facilitate conflicts resolution, markers include various annotations
besides old and news changeset identifiers, such as creation date or
author name.


Format
------

Markers are stored in an append-only file stored in
'.hg/store/obsstore'.

The file starts with a version header:

- 1 unsigned byte: version number, starting at zero.


The header is followed by the markers. Each marker is made of:

- 1 unsigned byte: number of new changesets "R", could be zero.

- 1 unsigned 32-bits integer: metadata size "M" in bytes.

- 1 byte: a bit field. It is reserved for flags used in obsolete
  markers common operations, to avoid repeated decoding of metadata
  entries.

- 20 bytes: obsoleted changeset identifier.

- N*20 bytes: new changesets identifiers.

- M bytes: metadata as a sequence of nul-terminated strings. Each
  string contains a key and a value, separated by a color ':', without
  additional encoding. Keys cannot contain '\0' or ':' and values
  cannot contain '\0'.
"""
import struct
from mercurial import util, base85
from i18n import _

_pack = struct.pack
_unpack = struct.unpack



# data used for parsing and writing
_fmversion = 0
_fmfixed   = '>BIB20s'
_fmnode = '20s'
_fmfsize = struct.calcsize(_fmfixed)
_fnodesize = struct.calcsize(_fmnode)

def _readmarkers(data):
    """Read and enumerate markers from raw data"""
    off = 0
    diskversion = _unpack('>B', data[off:off + 1])[0]
    off += 1
    if diskversion != _fmversion:
        raise util.Abort(_('parsing obsolete marker: unknown version %r')
                         % diskversion)

    # Loop on markers
    l = len(data)
    while off + _fmfsize <= l:
        # read fixed part
        cur = data[off:off + _fmfsize]
        off += _fmfsize
        nbsuc, mdsize, flags, pre = _unpack(_fmfixed, cur)
        # read replacement
        sucs = ()
        if nbsuc:
            s = (_fnodesize * nbsuc)
            cur = data[off:off + s]
            sucs = _unpack(_fmnode * nbsuc, cur)
            off += s
        # read metadata
        # (metadata will be decoded on demand)
        metadata = data[off:off + mdsize]
        if len(metadata) != mdsize:
            raise util.Abort(_('parsing obsolete marker: metadata is too '
                               'short, %d bytes expected, got %d')
                             % (len(metadata), mdsize))
        off += mdsize
        yield (pre, sucs, flags, metadata)

def encodemeta(meta):
    """Return encoded metadata string to string mapping.

    Assume no ':' in key and no '\0' in both key and value."""
    for key, value in meta.iteritems():
        if ':' in key or '\0' in key:
            raise ValueError("':' and '\0' are forbidden in metadata key'")
        if '\0' in value:
            raise ValueError("':' are forbidden in metadata value'")
    return '\0'.join(['%s:%s' % (k, meta[k]) for k in sorted(meta)])

def decodemeta(data):
    """Return string to string dictionary from encoded version."""
    d = {}
    for l in data.split('\0'):
        if l:
            key, value = l.split(':')
            d[key] = value
    return d

class marker(object):
    """Wrap obsolete marker raw data"""

    def __init__(self, repo, data):
        # the repo argument will be used to create changectx in later version
        self._repo = repo
        self._data = data
        self._decodedmeta = None

    def precnode(self):
        """Precursor changeset node identifier"""
        return self._data[0]

    def succnodes(self):
        """List of successor changesets node identifiers"""
        return self._data[1]

    def metadata(self):
        """Decoded metadata dictionary"""
        if self._decodedmeta is None:
            self._decodedmeta = decodemeta(self._data[3])
        return self._decodedmeta

    def date(self):
        """Creation date as (unixtime, offset)"""
        parts = self.metadata()['date'].split(' ')
        return (float(parts[0]), int(parts[1]))

class obsstore(object):
    """Store obsolete markers

    Markers can be accessed with two mappings:
    - precursors: old -> set(new)
    - successors: new -> set(old)
    """

    def __init__(self, sopener):
        self._all = []
        # new markers to serialize
        self.precursors = {}
        self.successors = {}
        self.sopener = sopener
        data = sopener.tryread('obsstore')
        if data:
            for marker in _readmarkers(data):
                self._load(marker)

    def __iter__(self):
        return iter(self._all)

    def __nonzero__(self):
        return bool(self._all)

    def create(self, transaction, prec, succs=(), flag=0, metadata=None):
        """obsolete: add a new obsolete marker

        * ensuring it is hashable
        * check mandatory metadata
        * encode metadata
        """
        if metadata is None:
            metadata = {}
        if len(prec) != 20:
            raise ValueError(prec)
        for succ in succs:
            if len(succ) != 20:
                raise ValueError(succ)
        marker = (str(prec), tuple(succs), int(flag), encodemeta(metadata))
        self.add(transaction, marker)

    def add(self, transaction, marker):
        """Add a new marker to the store"""
        if marker not in self._all:
            f = self.sopener('obsstore', 'ab')
            try:
                # Whether the file's current position is at the begin or at
                # the end after opening a file for appending is implementation
                # defined. So we must seek to the end before calling tell(),
                # or we may get a zero offset for non-zero sized files on
                # some platforms (issue3543).
                f.seek(0, 2) # os.SEEK_END
                offset = f.tell()
                transaction.add('obsstore', offset)
                if offset == 0:
                    # new file add version header
                    f.write(_pack('>B', _fmversion))
                _writemarkers(f.write, [marker])
            finally:
                # XXX: f.close() == filecache invalidation == obsstore rebuilt.
                # call 'filecacheentry.refresh()'  here
                f.close()
            self._load(marker)

    def mergemarkers(self, transation, data):
        other = _readmarkers(data)
        local = set(self._all)
        new = [m for m in other if m not in local]
        for marker in new:
            # XXX: N marker == N x (open, write, close)
            # we should write them all at once
            self.add(transation, marker)

    def _load(self, marker):
        self._all.append(marker)
        pre, sucs = marker[:2]
        self.precursors.setdefault(pre, set()).add(marker)
        for suc in sucs:
            self.successors.setdefault(suc, set()).add(marker)

def _writemarkers(write, markers):
    # Kept separate from flushmarkers(), it will be reused for
    # markers exchange.
    for marker in markers:
        pre, sucs, flags, metadata = marker
        nbsuc = len(sucs)
        format = _fmfixed + (_fmnode * nbsuc)
        data = [nbsuc, len(metadata), flags, pre]
        data.extend(sucs)
        write(_pack(format, *data))
        write(metadata)

def listmarkers(repo):
    """List markers over pushkey"""
    if not repo.obsstore:
        return {}
    data = [_pack('>B', _fmversion)]
    _writemarkers(data.append, repo.obsstore)
    return {'dump': base85.b85encode(''.join(data))}

def pushmarker(repo, key, old, new):
    """Push markers over pushkey"""
    if key != 'dump':
        repo.ui.warn(_('unknown key: %r') % key)
        return 0
    if old:
        repo.ui.warn(_('unexpected old value') % key)
        return 0
    data = base85.b85decode(new)
    lock = repo.lock()
    try:
        tr = repo.transaction('pushkey: obsolete markers')
        try:
            repo.obsstore.mergemarkers(tr, data)
            tr.close()
            return 1
        finally:
            tr.release()
    finally:
        lock.release()

def allmarkers(repo):
    """all obsolete markers known in a repository"""
    for markerdata in repo.obsstore:
        yield marker(repo, markerdata)

def precursormarkers(ctx):
    """obsolete marker making this changeset obsolete"""
    for data in ctx._repo.obsstore.precursors.get(ctx.node(), ()):
        yield marker(ctx._repo, data)

def successormarkers(ctx):
    """obsolete marker marking this changeset as a successors"""
    for data in ctx._repo.obsstore.successors.get(ctx.node(), ()):
        yield marker(ctx._repo, data)

def anysuccessors(obsstore, node):
    """Yield every successor of <node>

    This this a linear yield unsuitable to detect splitted changeset."""
    remaining = set([node])
    seen = set(remaining)
    while remaining:
        current = remaining.pop()
        yield current
        for mark in obsstore.precursors.get(current, ()):
            for suc in mark[1]:
                if suc not in seen:
                    seen.add(suc)
                    remaining.add(suc)
