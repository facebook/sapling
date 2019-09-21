# bundleparts.py - utilies to pack/unpack the snapshot metadata into bundles
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from edenscm.mercurial import bundle2, error
from edenscm.mercurial.i18n import _
from edenscm.mercurial.utils import cborutil

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
            raise error.Abort(_("%s not found in repo") % rev)
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
    store = op.repo.svfs.snapshotstore
    for oid, data in binarydecode(inpart):
        store.write(oid, data)


def binaryencode(repo, metadataids):
    """encode snapshot metadata into a binary CBOR stream

    format (CBOR-encoded):
    {
        "metadatafiles": {
            <metadata oid>: <binary metadata content>,
            . . .
        },
        "auxfiles": {
            <file oid>: <binary file content>,
            . . .
        }
    }
    """

    metadataauxfilesinfo = set()
    bundlepartdict = {"metadatafiles": {}, "auxfiles": {}}
    store = repo.svfs.snapshotstore
    # store the metadata files
    for metadataid in metadataids:
        data = store.read(metadataid)
        bundlepartdict["metadatafiles"][metadataid] = data
        snapmetadata = metadata.snapshotmetadata.deserialize(data)
        metadataauxfilesinfo.update(snapmetadata.getauxfilesinfo())
    # store files that are mentioned in metadata
    for fileid in metadataauxfilesinfo:
        bundlepartdict["auxfiles"][fileid] = store.read(fileid)
    return "".join(cborutil.streamencode(bundlepartdict))


def binarydecode(stream):
    """decode a binary CBOR stream into individual blobs and store them
    Generates pairs of (oid, data).

    format (CBOR-encoded):
    {
        "metadatafiles": {
            <metadata oid>: <binary metadata content>,
            . . .
        },
        "auxfiles": {
            <file oid>: <binary file content>,
            . . .
        }
    }
    """

    try:
        bundlepartdict = cborutil.decodeall(stream.read())[0]
    except cborutil.CBORDecodeError:
        raise error.Abort(_("invalid bundlepart stream"))
    try:
        for section in ("metadatafiles", "auxfiles"):
            for oid, content in bundlepartdict[section].iteritems():
                yield oid, content
    except (KeyError, ValueError):
        raise error.Abort(_("invalid bundlepart dict: %s") % (bundlepartdict,))
