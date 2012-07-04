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
        self._new = []
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

    def create(self, prec, succs=(), flag=0, metadata=None):
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
        self.add(marker)

    def add(self, marker):
        """Add a new marker to the store

        This marker still needs to be written to disk"""
        self._new.append(marker)
        self._load(marker)

    def mergemarkers(self, data):
        other = set(_readmarkers(data))
        local = set(self._all)
        new = other - local
        for marker in new:
            self.add(marker)

    def flushmarkers(self):
        """Write all markers on disk

        After this operation, "new" markers are considered "known"."""
        if self._new:
            # XXX: transaction logic should be used here. But for
            # now rewriting the whole file is good enough.
            f = self.sopener('obsstore', 'wb', atomictemp=True)
            try:
                self._writemarkers(f)
                f.close()
                self._new[:] = []
            except: # re-raises
                f.discard()
                raise

    def _load(self, marker):
        self._all.append(marker)
        pre, sucs = marker[:2]
        self.precursors.setdefault(pre, set()).add(marker)
        for suc in sucs:
            self.successors.setdefault(suc, set()).add(marker)

    def _writemarkers(self, stream=None):
        # Kept separate from flushmarkers(), it will be reused for
        # markers exchange.
        if stream is None:
            final = []
            w = final.append
        else:
            w = stream.write
        w(_pack('>B', _fmversion))
        for marker in self._all:
            pre, sucs, flags, metadata = marker
            nbsuc = len(sucs)
            format = _fmfixed + (_fmnode * nbsuc)
            data = [nbsuc, len(metadata), flags, pre]
            data.extend(sucs)
            w(_pack(format, *data))
            w(metadata)
        if stream is None:
            return ''.join(final)

def listmarkers(repo):
    """List markers over pushkey"""
    if not repo.obsstore:
        return {}
    data = repo.obsstore._writemarkers()
    return {'dump': base85.b85encode(data)}

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
        repo.obsstore.mergemarkers(data)
        return 1
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

