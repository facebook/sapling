# coding=UTF-8

from __future__ import absolute_import

from mercurial import (
    revlog,
    util as hgutil,
)
from mercurial.i18n import _
from mercurial.node import bin, nullid

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
    if self.opener.options['lfsbypass']:
        return (text, False)

    metadata = pointer.deserialize(text)
    storeids = metadata.tostoreids()
    store = self.opener.lfslocalblobstore
    if not isinstance(storeids, list):
        storeids = [storeids]
    missing = filter(lambda id: not store.has(id), storeids)
    if missing:
        self.opener.lfsremoteblobstore.readbatch(missing, store)
    text = ''.join([store.read(id) for id in storeids])
    return (text, True)

def writetostore(self, text):
    if self.opener.options['lfsbypass']:
        return (text, False)

    offset = 0
    chunkoids = []
    chunksize = self.opener.options['lfschunksize']

    if not chunksize:
        chunksize = len(text)

    while offset < len(text):
        chunk = text[offset:offset + chunksize]  # Python handles overflows
        chunklen = len(chunk)
        # compute sha256 for git-lfs
        sha = util.sha256(chunk)
        # Store actual contents to local blobstore
        storeid = blobstore.StoreID(sha, chunklen)
        self.opener.lfslocalblobstore.write(storeid, chunk)
        chunkoids.append(storeid)
        offset += chunklen

    # replace contents with metadata
    metadata = pointer.ChunkingPointer(
        chunks=[{'oid': v.oid, 'size': v.size} for v in chunkoids],
        hashalgo='sha256',
        size=len(text))

    # hg filelog metadata (includes rename, etc)
    hgmeta = getattr(self, '_filelogmeta', None)
    if hgmeta:
        # only care about a whitelist of hg filelog metadata
        for name in ['copy', 'copyrev']:
            if name in hgmeta:
                metadata['x-hg-%s' % name] = hgmeta[name]
    text = str(metadata)

    return (text, False)

def _islfs(rlog, node):
    if node == nullid:
        return False
    rev = rlog.rev(node)
    flags = revlog.revlog.flags(rlog, rev)
    return bool(flags & revlog.REVIDX_EXTSTORED)

def filelogadd(orig, self, text, meta, transaction, link, p1=None, p2=None):
    # drop meta (usually used for renaming tracking), to simplify blob handling
    if not self.opener.options['lfsbypass']:
        threshold = self.opener.options['lfsthreshold']

        if threshold and len(text) > threshold:
            flags = revlog.REVIDX_EXTSTORED | revlog.REVIDX_DEFAULT_FLAGS
            self._filelogmeta = meta # for flagprocessor to pick up
            try:
                return self.addrevision(text, transaction, link, p1, p2,
                                        flags=flags)
            finally:
                self._filelogmeta = None

    return orig(self, text, meta, transaction, link, p1, p2)

def filelogread(orig, self, node):
    if _islfs(self, node):
        # no metadata stored, no need to test metadata header ("\1\n")
        return self.revision(node)
    return orig(self, node)

def filelogcmp(orig, self, node, text):
    if text.startswith('\1\n') and _islfs(self, node):
        # do not prepend '\1\n' in lfs's case, test directly
        return self.revision(node) != text
    return orig(self, node, text)

def filelogrenamed(orig, self, node):
    if _islfs(self, node):
        rawtext = self.revision(node, raw=True)
        if not rawtext:
            return False
        metadata = pointer.deserialize(rawtext)
        if 'x-hg-copy' in metadata and 'x-hg-copyrev' in metadata:
            return metadata['x-hg-copy'], bin(metadata['x-hg-copyrev'])
        else:
            return False
    return orig(self, node)

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

    if ui.verbose:
        ui.write(_('lfs: computing set of blobs to upload\n'))
    toupload = []
    totalsize = 0
    for i, n in enumerate(pushop.outgoing.missing):
        ctx = repo[n]
        files = set(ctx.files())
        parents = [p for p in ctx.parents() if p != nullid]
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
            if f not in ctx:
                continue
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

    if not toupload:
        return

    if ui.verbose:
        msg = _('lfs: uploading blobs to the remote (%s chunk(s), %s)\n')
        ui.write(msg % (len(toupload), hgutil.bytecount(totalsize)))

    remoteblob = repo.svfs.lfsremoteblobstore
    remoteblob.writebatch(toupload, repo.svfs.lfslocalblobstore,
                          total=totalsize)
