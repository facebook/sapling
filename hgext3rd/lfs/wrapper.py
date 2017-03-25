# coding=UTF-8

from __future__ import absolute_import

from mercurial import (
    node,
    revlog,
    util as hgutil,
)
from mercurial.i18n import _

from . import (
    blobstore,
    pointer,
    util,
)

def supportedoutgoingversions(orig, repo):
    versions = orig(repo)
    versions.discard('01')
    versions.discard('02')
    versions.add('03')
    return versions

def allsupportedversions(orig, ui):
    versions = orig(ui)
    versions.add('03')
    return versions

def bypasscheckhash(self, text):
    return False

def readfromstore(self, text):
    """Read filelog content from local blobstore transform for flagprocessor.

    Default tranform for flagprocessor, returning contents from blobstore.
    Returns a 2-typle (text, validatehash) where validatehash is True as the
    contents of the blobstore should be checked using checkhash.
    """
    try:
        metadata = pointer.deserialize(text)
        storeids = metadata.tostoreids()
        store = blobstore.local.get(self.opener)
        if not isinstance(storeids, list):
            storeids = [storeids]
        missing = filter(lambda id: not store.has(id), storeids)
        if missing:
            blobstore.remote.get(self.opener).readbatch(missing, store)
        text = ''.join([store.read(id) for id in storeids])
        return (text, True)
    except Exception:
        return (text), True

def writetostore(self, text):
    offset = 0
    chunkoids = []
    chunksize = util.getoption(self.opener, 'lfschunksize')

    if not chunksize:
        chunksize = len(text)

    while offset < len(text):
        chunk = text[offset:offset + chunksize]  # Python handles overflows
        chunklen = len(chunk)
        # compute sha256 for git-lfs
        sha = util.sha256(chunk)
        # Store actual contents to local blobstore
        storeid = blobstore.StoreID(sha, chunklen)
        blobstore.local.get(self.opener).write(storeid, chunk)
        chunkoids.append(storeid)
        offset += chunklen

    # replace contents with metadata
    metadata = pointer.ChunkingPointer(
        chunks=[{'oid': v.oid, 'size': v.size} for v in chunkoids],
        hashalgo='sha256',
        size=len(text))
    text = str(metadata)

    return (text, False)

def addrevision(orig, self, text, transaction, link, p1, p2, cachedelta=None,
                node=None, flags=revlog.REVIDX_DEFAULT_FLAGS):
    """filelog.addrevision wrapper.
    FIXME
    """
    threshold = util.getoption(self.opener, 'lfsthreshold')

    if threshold and len(text) > threshold:
        flags |= revlog.REVIDX_EXTSTORED

    return orig(self, text, transaction, link, p1, p2, cachedelta=cachedelta,
                node=node, flags=flags)

def prepush(pushop):
    """Prepush hook.

    Read through the revisions to push, looking for filelog entries that can be
    deserialized into metadata so that we can block the push on their upload to
    the remote blobstore.
    """
    repo = pushop.repo
    ui = pushop.ui
    remoterepo = pushop.remote.local()

    # We beed to pass on the information to the remote about the threshold so
    # that _peek_islargefile can mark the file as large file.
    threshold = repo.svfs.options.get('lfsthreshold')
    if threshold is not None:
        remoterepo.svfs.options['lfsthreshold'] = threshold

    ui.write(_('lfs: computing set of blobs to upload\n'))
    toupload = []
    totalsize = 0
    for i, n in enumerate(pushop.outgoing.missing):
        ctx = repo[n]
        files = set(ctx.files())
        parents = [p for p in ctx.parents() if p != node.nullid]
        if len(parents) == 2:
            mc = ctx.manifest()
            mp1 = ctx.parents()[0].manifest()
            mp2 = ctx.parents()[1].manifest()
            for f in mp1:
                if f not in mc:
                    files.add(f)
            for f in mp2:
                if f not in mc:
                    files.add(f)
            for f in mc:
                if mc[f] != mp1.get(f, None) or mc[f] != mp2.get(f, None):
                    files.add(f)

        for f in files:
            filectx = ctx[f]
            flags = filectx.filelog().flags(filectx.filerev())
            if flags & revlog.REVIDX_EXTSTORED != revlog.REVIDX_EXTSTORED:
                continue
            try:
                metadata = pointer.deserialize(ctx[f].rawdata())
                totalsize += long(metadata['size'])
                storeids = metadata.tostoreids()
                if isinstance(storeids, list):
                    toupload.extend(storeids)
                else:
                    toupload.append(storeids)
            except pointer.PointerDeserializationError:
                msg = _('lfs: could not deserialize pointer for file %s, '
                        'revision %s\n')
                ui.write(msg % (f, filectx.filerev()))
                raise

    remoteblob = blobstore.remote.get(repo.svfs)
    msg = _('lfs: uploading the blobs to the remote (%s chunk(s), %s)\n')
    ui.write(msg % (len(toupload), hgutil.bytecount(totalsize)))
    remoteblob.writebatch(toupload, blobstore.local.get(repo.svfs),
                          total=totalsize)
