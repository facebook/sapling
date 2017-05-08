# coding=UTF-8

from __future__ import absolute_import

from mercurial import (
    error,
    filelog,
    revlog,
    util as hgutil,
)
from mercurial.i18n import _
from mercurial.node import bin, nullid, short

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
        storeids = [metadata.tostoreid()]
        store = self.opener.lfslocalblobstore
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

    # compute sha256 for git-lfs
    sha = util.sha256(text)
    # Store actual contents to local blobstore
    storeid = blobstore.StoreID(sha, len(text))
    self.opener.lfslocalblobstore.write(storeid, text)

    # replace contents with metadata
    hashalgo = 'sha256'
    metadata = pointer.GithubPointer(storeid.oid, hashalgo, storeid.size)

    # by default, we expect the content to be binary. however, LFS could also
    # be used for non-binary content. add a special entry for non-binary data.
    # this will be used by filectx.isbinary().
    if not hgutil.binary(text):
        # not hg filelog metadata (affecting commit hash), no "x-hg-" prefix
        metadata['x-is-binary'] = '0'

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
    flags = rlog.flags(rev)
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

def filelogsize(orig, self, rev):
    if _islfs(self, rev=rev):
        # fast path: use lfs metadata to answer size
        rawtext = self.revision(rev, raw=True)
        metadata = pointer.deserialize(rawtext)
        return int(metadata['size'])
    return orig(self, rev)

def filectxisbinary(orig, self):
    flog = self.filelog()
    node = self.filenode()
    if _islfs(flog, node):
        # fast path: use lfs metadata to answer isbinary
        rawtext = flog.revision(node, raw=True)
        metadata = pointer.deserialize(rawtext)
        # if lfs metadata says nothing, assume it's binary by default
        return bool(int(metadata['x-is-binary'] or 1))
    return orig(self)

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
    pointers = extractpointers(pushop.repo, pushop.outgoing.missing)
    uploadblobs(pushop.repo, [p.tostoreid() for p in pointers])

def extractpointers(repo, revs):
    """return a list of lfs pointers added by given revs"""
    ui = repo.ui
    if ui.verbose:
        ui.write(_('lfs: computing set of blobs to upload\n'))
    pointers = {}
    for i, n in enumerate(revs):
        ctx = repo[n]
        files = set(ctx.files())
        for f in files:
            if f not in ctx:
                continue
            fctx = ctx[f]
            if not _islfs(fctx.filelog(), fctx.filenode()):
                continue
            try:
                metadata = pointer.deserialize(fctx.rawdata())
                pointers[metadata['oid']] = metadata
            except pointer.PointerDeserializationError:
                raise error.Abort(_('lfs: corrupted pointer (%s@%s)\n')
                                  % (f, short(ctx.node())))
    return pointers.values()

def uploadblobs(repo, storeids):
    """upload given storeids from local blobstore"""
    if not storeids:
        return

    totalsize = sum(s.size for s in storeids)

    ui = repo.ui
    if ui.verbose:
        msg = _('lfs: need to upload %s objects (%s)\n')
        ui.write(msg % (len(storeids), hgutil.bytecount(totalsize)))

    remoteblob = repo.svfs.lfsremoteblobstore
    remoteblob.writebatch(storeids, repo.svfs.lfslocalblobstore,
                          total=totalsize)
