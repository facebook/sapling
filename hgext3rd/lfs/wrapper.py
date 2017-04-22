# coding=UTF-8

from __future__ import absolute_import

from mercurial import (
    filelog,
    revlog,
    util as hgutil,
)
from mercurial.i18n import _
from mercurial.node import nullid

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
    metadata = pointer.deserialize(text)
    verifyhash = False

    # if bypass is set, do not read remote blobstore, skip hash check, but
    # still write hg filelog metadata
    if not self.opener.options['lfsbypass']:
        verifyhash = True
        storeids = metadata.tostoreids()
        store = self.opener.lfslocalblobstore
        if not isinstance(storeids, list):
            storeids = [storeids]
        missing = filter(lambda id: not store.has(id), storeids)
        if missing:
            self.opener.lfsremoteblobstore.readbatch(missing, store)
        text = ''.join([store.read(id) for id in storeids])

    # pack hg filelog metadata
    hgmeta = {}
    for k in metadata.keys():
        if k.startswith('x-hg-'):
            name = k[len('x-hg-'):]
            hgmeta[name] = metadata[k]
    if hgmeta or text.startswith('\1\n'):
        text = filelog.packmeta(hgmeta, text)

    return (text, verifyhash)

def writetostore(self, text):
    if self.opener.options['lfsbypass']:
        return (text, False)

    # hg filelog metadata (includes rename, etc)
    hgmeta, offset = filelog.parsemeta(text)
    if offset and offset > 0:
        # lfs blob does not contain hg filelog metadata
        text = text[offset:]

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
    hashalgo = 'sha256'
    if len(chunkoids) == 1:
        storeid = chunkoids[0]
        metadata = pointer.GithubPointer(storeid.oid, hashalgo, storeid.size)
    else:
        metadata = pointer.ChunkingPointer(
            chunks=[{'oid': v.oid, 'size': v.size} for v in chunkoids],
            hashalgo=hashalgo,
            size=len(text))

    # translate hg filelog metadata to lfs metadata with "x-hg-" prefix
    if hgmeta is not None:
        for k, v in hgmeta.iteritems():
            metadata['x-hg-%s' % k] = v
    text = str(metadata)

    return (text, False)

def _islfs(rlog, node=None, rev=None):
    if rev is None:
        rev = rlog.rev(node)
    else:
        node = rlog.node(rev)
    if node == nullid:
        return False
    flags = revlog.revlog.flags(rlog, rev)
    return bool(flags & revlog.REVIDX_EXTSTORED)

def filelogaddrevision(orig, self, text, transaction, link, p1, p2,
                       cachedelta=None, node=None,
                       flags=revlog.REVIDX_DEFAULT_FLAGS, **kwds):
    if not self.opener.options['lfsbypass']:
        threshold = self.opener.options['lfsthreshold']
        textlen = len(text)
        # exclude hg rename meta from file size
        meta, offset = filelog.parsemeta(text)
        if offset:
            textlen -= offset

        if threshold and textlen > threshold:
            flags |= revlog.REVIDX_EXTSTORED

    return orig(self, text, transaction, link, p1, p2, cachedelta=cachedelta,
                node=node, flags=flags, **kwds)

def filelogsize(orig, self, rev):
    if _islfs(self, rev=rev):
        # fast path: use lfs metadata to answer size
        rawtext = self.revision(rev, raw=True)
        metadata = pointer.deserialize(rawtext)
        return int(metadata['size'])
    return orig(self, rev)

def vfsinit(orig, self, othervfs):
    orig(self, othervfs)
    # copy lfs related options
    for k, v in othervfs.options.items():
        if k.startswith('lfs'):
            self.options[k] = v
    # also copy lfs blobstores. note: this can run before reposetup, so lfs
    # blobstore attributes are not always ready at this time.
    for name in ['lfslocalblobstore', 'lfsremoteblobstore']:
        if hgutil.safehasattr(othervfs, name):
            setattr(self, name, getattr(othervfs, name))

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
        msg = _('lfs: need to upload %s objects (%s)\n')
        ui.write(msg % (len(toupload), hgutil.bytecount(totalsize)))

    remoteblob = repo.svfs.lfsremoteblobstore
    remoteblob.writebatch(toupload, repo.svfs.lfslocalblobstore,
                          total=totalsize)
