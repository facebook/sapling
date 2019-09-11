# bundleparts.py - utilies to pack/unpack the snapshot metadata into bundles
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import struct

from edenscm.mercurial import bundle2, error
from edenscm.mercurial.i18n import _

from . import metadata


snapshotmetadataparttype = "b2x:snapshotmetadata"


def uisetup(ui):
    if ui.configbool("snapshot", "enable-sync-bundle"):
        bundle2.capabilities[snapshotmetadataparttype] = ()


def getmetadatafromrevs(repo, revs):
    """get binary representation of snapshot metadata by a list of revs
    """
    metadataids = set()
    unfi = repo.unfiltered()
    for rev in revs:
        # TODO(alexeyqu): move this check into a function
        if rev not in unfi:
            raise error.Abort(_("%s not found in repo\n") % rev)
        ctx = unfi[rev]
        snapshotmetadataid = ctx.extra().get("snapshotmetadataid", None)
        if snapshotmetadataid:
            metadataids.add(snapshotmetadataid)
    if not metadataids:
        return None
    return binaryencode(repo, metadataids)


@bundle2.parthandler(snapshotmetadataparttype)
def handlemetadata(op, inpart):
    """unpack metadata for snapshots
    """
    binarydecode(op.repo, inpart)


_versionentry = struct.Struct(">B")
_binaryentry = struct.Struct(">64sI")


def binaryencode(repo, metadataids):
    """encode snapshot metadata into a binary stream

    the binary format is:
        <version-byte>[<chunk-id><chunk-length><chunk-content>]+

    :version-byte: is a version byte
    :chunk-id: is a string of 64 chars -- sha256 of the chunk
    :chunk-length: is an unsigned int
    :chunk-content: is the metadata contents (of length <chunk-length>)
    """

    def _encode(oid, data):
        return [_binaryentry.pack(oid, len(data)), data]

    metadataauxfileids = set()
    binaryparts = []
    # store the version info
    binaryparts.append(_versionentry.pack(metadata.snapshotmetadata.VERSION))
    # store the metadata files
    for metadataid in metadataids:
        snapmetadata = metadata.snapshotmetadata.getfromlocalstorage(repo, metadataid)
        metadataauxfileids.update(snapmetadata.getauxfileids())
        data = snapmetadata.serialize()
        binaryparts += _encode(metadataid, data)
    # store files that are mentioned in metadata
    for auxfileid in metadataauxfileids:
        data = repo.svfs.snapshotstore.read(auxfileid)
        binaryparts += _encode(auxfileid, data)
    return "".join(binaryparts)


def binarydecode(repo, stream):
    """decode a binary stream into individual blobs and store them
    Returns a list of file ids.

    the binary format is:
        <version-byte>[<chunk-id><chunk-length><chunk-content>]+

    :version-byte: is a version byte
    :chunk-id: is a string of 64 chars -- sha256 of the chunk
    :chunk-length: is an unsigned int
    :chunk-content: is the metadata contents (of length <chunk-length>)
    """
    # check the version info
    version = _versionentry.unpack(stream.read(_versionentry.size))[0]
    if version != metadata.snapshotmetadata.VERSION:
        raise error.Abort(_("invalid version number %d") % version)
    entrysize = _binaryentry.size
    fileids = []
    while True:
        entry = stream.read(entrysize)
        if len(entry) < entrysize:
            if entry:
                raise error.Abort(_("bad snapshot metadata stream"))
            break
        oid, length = _binaryentry.unpack(entry)
        data = stream.read(length)
        if len(data) < length:
            if data:
                raise error.Abort(_("bad snapshot metadata stream"))
        repo.svfs.snapshotstore.write(oid, data)
        fileids.append(oid)
    return fileids
