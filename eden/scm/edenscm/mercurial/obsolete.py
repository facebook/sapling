# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# obsolete.py - obsolete markers handling
#
# Copyright 2012 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Obsolete marker handling

An obsolete marker maps an old changeset to a list of new
changesets. If the list of new changesets is empty, the old changeset
is said to be "killed". Otherwise, the old changeset is being
"replaced" by the new changesets.

Obsolete markers can be used to record and distribute changeset graph
transformations performed by history rewrite operations, and help
building new tools to reconcile conflicting rewrite actions. To
facilitate conflict resolution, markers include various annotations
besides old and news changeset identifiers, such as creation date or
author name.

The old obsoleted changeset is called a "predecessor" and possible
replacements are called "successors". Markers that used changeset X as
a predecessor are called "successor markers of X" because they hold
information about the successors of X. Markers that use changeset Y as
a successors are call "predecessor markers of Y" because they hold
information about the predecessors of Y.

Examples:

- When changeset A is replaced by changeset A', one marker is stored:

    (A, (A',))

- When changesets A and B are folded into a new changeset C, two markers are
  stored:

    (A, (C,)) and (B, (C,))

- When changeset A is simply "pruned" from the graph, a marker is created:

    (A, ())

- When changeset A is split into B and C, a single marker is used:

    (A, (B, C))

  We use a single marker to distinguish the "split" case from the "divergence"
  case. If two independent operations rewrite the same changeset A in to A' and
  A'', we have an error case: divergent rewriting. We can detect it because
  two markers will be created independently:

  (A, (B,)) and (A, (C,))

"""
from __future__ import absolute_import

import struct

from edenscmnative import parsers

from . import error, util
from .i18n import _
from .pycompat import encodeutf8


_pack = struct.pack
_unpack = struct.unpack
_calcsize = struct.calcsize
propertycache = util.propertycache

## Parsing and writing of version "1"
#
# The header is followed by the markers. Each marker is made of:
#
# - uint32: total size of the marker (including this field)
#
# - float64: date in seconds since epoch
#
# - int16: timezone offset in minutes
#
# - uint16: a bit field. It is reserved for flags used in common
#   obsolete marker operations, to avoid repeated decoding of metadata
#   entries.
#
# - uint8: number of successors "N", can be zero.
#
# - uint8: number of parents "P", can be zero.
#
#     0: parents data stored but no parent,
#     1: one parent stored,
#     2: two parents stored,
#     3: no parent data stored
#
# - uint8: number of metadata entries M
#
# - 20 or 32 bytes: predecessor changeset identifier.
#
# - N*(20 or 32) bytes: successors changesets identifiers.
#
# - P*(20 or 32) bytes: parents of the predecessors changesets.
#
# - M*(uint8, uint8): size of all metadata entries (key and value)
#
# - remaining bytes: the metadata, each (key, value) pair after the other.
_fm1version = 1
_fm1fixed = ">IdhHBBB20s"
_fm1nodesha1 = "20s"
_fm1parentnone = 3
_fm1metapair = "BB"


def _fm1encodeonemarker(marker):
    pre, sucs, flags, metadata, date, parents = marker
    # determine node size
    _fm1node = _fm1nodesha1
    numsuc = len(sucs)
    numextranodes = numsuc
    if parents is None:
        numpar = _fm1parentnone
    else:
        numpar = len(parents)
        numextranodes += numpar
    formatnodes = _fm1node * numextranodes
    formatmeta = _fm1metapair * len(metadata)
    format = _fm1fixed + formatnodes + formatmeta
    # tz is stored in minutes so we divide by 60
    tz = date[1] // 60
    data = [None, date[0], tz, flags, numsuc, numpar, len(metadata), pre]
    data.extend(sucs)
    if parents is not None:
        data.extend(parents)
    totalsize = _calcsize(format)
    for key, value in metadata:
        assert isinstance(key, str)
        assert isinstance(value, str)
        lk = len(key)
        lv = len(value)
        if lk > 255:
            msg = (
                "obsstore metadata key cannot be longer than 255 bytes"
                ' (key "%s" is %u bytes)'
            ) % (key, lk)
            raise error.ProgrammingError(msg)
        if lv > 255:
            msg = (
                "obsstore metadata value cannot be longer than 255 bytes"
                ' (value "%s" for key "%s" is %u bytes)'
            ) % (value, key, lv)
            raise error.ProgrammingError(msg)
        data.append(lk)
        data.append(lv)
        totalsize += lk + lv
    data[0] = totalsize
    data = [_pack(format, *data)]
    for key, value in metadata:
        key = encodeutf8(key)
        value = encodeutf8(value)
        data.append(key)
        data.append(value)
    return b"".join(data)


# mapping to read/write various marker formats
# <version> -> (decoder, encoder)
formats = {
    # pyre-fixme[16]: Module `parsers` has no attribute `fm1readmarkers`.
    _fm1version: (parsers.fm1readmarkers, _fm1encodeonemarker),
}


def _readmarkerversion(data):
    return _unpack(">B", data[0:1])[0]


@util.nogc
def _readmarkers(data, off=None, stop=None):
    """Read and enumerate markers from raw data"""
    diskversion = _readmarkerversion(data)
    if not off:
        off = 1  # skip 1 byte version number
    if stop is None:
        stop = len(data)
    if diskversion not in formats:
        msg = _("parsing obsolete marker: unknown version %r") % diskversion
        raise error.UnknownVersion(msg, version=diskversion)
    return diskversion, formats[diskversion][0](data, off, stop)


def encodeheader(version=_fm1version):
    return _pack(">B", version)


def encodemarkers(markers, addheader=False, version=_fm1version):
    # Kept separate from flushmarkers(), it will be reused for
    # markers exchange.
    encodeone = formats[version][1]
    if addheader:
        yield encodeheader(version)
    for marker in markers:
        yield encodeone(marker)


def commonversion(versions):
    """Return the newest version listed in both versions and our local formats.

    Returns None if no common version exists.
    """
    versions.sort(reverse=True)
    # search for highest version known on both side
    for v in versions:
        if v in formats:
            return v
    return None
