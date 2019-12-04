# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# wrapper.py - methods wrapping core mercurial logic

from __future__ import absolute_import

import hashlib

from edenscm.mercurial import error, filelog, revlog, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, nullid, short

from . import blobstore, pointer


def supportedoutgoingversions(orig, repo):
    versions = orig(repo)
    versions.discard("01")
    versions.discard("02")
    versions.add("03")
    return versions


def allsupportedversions(orig, ui):
    versions = orig(ui)
    versions.add("03")
    return versions


def bypasscheckhash(self, text):
    return False


def readfromstore(self, text):
    """Read filelog content from local blobstore transform for flagprocessor.

    Default tranform for flagprocessor, returning contents from blobstore.
    Returns a 2-typle (text, validatehash) where validatehash is True as the
    contents of the blobstore should be checked using checkhash.
    """
    p = pointer.deserialize(text)
    oid = p.oid()
    store = self.opener.lfslocalblobstore
    if not store.has(oid):
        p.filename = getattr(self, "indexfile", None)
        self.opener.lfsremoteblobstore.readbatch([p], store)
    text = store.read(oid)

    # pack hg filelog metadata
    hgmeta = p.hgmeta()
    if hgmeta or text.startswith("\1\n"):
        text = filelog.packmeta(hgmeta, text)

    return (text, True)


def writetostore(self, text):
    # hg filelog metadata (includes rename, etc)
    hgmeta, offset = filelog.parsemeta(text)
    if offset and offset > 0:
        # lfs blob does not contain hg filelog metadata
        text = text[offset:]

    # git-lfs only supports sha256
    oid = hashlib.sha256(text).hexdigest()
    self.opener.lfslocalblobstore.write(oid, text)

    # replace contents with metadata
    longoid = "sha256:%s" % oid
    metadata = pointer.gitlfspointer(oid=longoid, size=str(len(text)))

    # by default, we expect the content to be binary. however, LFS could also
    # be used for non-binary content. add a special entry for non-binary data.
    # this will be used by filectx.isbinary().
    if not util.binary(text):
        # not hg filelog metadata (affecting commit hash), no "x-hg-" prefix
        metadata["x-is-binary"] = "0"

    # translate hg filelog metadata to lfs metadata with "x-hg-" prefix
    if hgmeta is not None:
        for k, v in hgmeta.iteritems():
            metadata["x-hg-%s" % k] = v

    rawtext = metadata.serialize()
    return (rawtext, False)


def _islfs(rlog, node=None, rev=None):
    if rev is None:
        if node is None:
            # both None - likely working copy content where node is not ready
            return False
        rev = rlog.rev(node)
    else:
        node = rlog.node(rev)
    if node == nullid:
        return False
    flags = rlog.flags(rev)
    return bool(flags & revlog.REVIDX_EXTSTORED)


def filelogaddrevision(
    orig,
    self,
    text,
    transaction,
    link,
    p1,
    p2,
    cachedelta=None,
    node=None,
    flags=revlog.REVIDX_DEFAULT_FLAGS,
    **kwds
):
    threshold = self.opener.options["lfsthreshold"]
    textlen = len(text)
    # exclude hg rename meta from file size
    meta, offset = filelog.parsemeta(text)
    if offset:
        textlen -= offset

    if threshold and textlen > threshold:
        flags |= revlog.REVIDX_EXTSTORED

    return orig(
        self,
        text,
        transaction,
        link,
        p1,
        p2,
        cachedelta=cachedelta,
        node=node,
        flags=flags,
        **kwds
    )


def filelogrenamed(orig, self, node):
    if _islfs(self, node):
        rawtext = self.revision(node, raw=True)
        if not rawtext:
            return False
        metadata = pointer.deserialize(rawtext)
        if "x-hg-copy" in metadata and "x-hg-copyrev" in metadata:
            return metadata["x-hg-copy"], bin(metadata["x-hg-copyrev"])
        else:
            return False
    return orig(self, node)


def filelogsize(orig, self, rev):
    if _islfs(self, rev=rev):
        # fast path: use lfs metadata to answer size
        rawtext = self.revision(rev, raw=True)
        metadata = pointer.deserialize(rawtext)
        return int(metadata["size"])
    return orig(self, rev)


def filectxcmp(orig, self, fctx):
    """returns True if text is different than fctx"""
    # some fctx (ex. hg-git) is not based on basefilectx and do not have islfs
    if self.islfs() and getattr(fctx, "islfs", lambda: False)():
        # fast path: check LFS oid
        p1 = pointer.deserialize(self.rawdata())
        p2 = pointer.deserialize(fctx.rawdata())
        return p1.oid() != p2.oid()
    return orig(self, fctx)


def filectxisbinary(orig, self):
    if self.islfs():
        # fast path: use lfs metadata to answer isbinary
        metadata = pointer.deserialize(self.rawdata())
        # if lfs metadata says nothing, assume it's binary by default
        return bool(int(metadata.get("x-is-binary", 1)))
    return orig(self)


def filectxislfs(self):
    return _islfs(self.filelog(), self.filenode())


def convertsink(orig, sink):
    sink = orig(sink)
    if sink.repotype == "hg":

        # pyre-fixme[11]: Annotation `__class__` is not defined as a type.
        class lfssink(sink.__class__):
            def putcommit(
                self, files, copies, parents, commit, source, revmap, full, cleanp2
            ):
                pc = super(lfssink, self).putcommit
                node = pc(files, copies, parents, commit, source, revmap, full, cleanp2)

                if "lfs" not in self.repo.requirements:
                    ctx = self.repo[node]

                    # The file list may contain removed files, so check for
                    # membership before assuming it is in the context.
                    if any(f in ctx and ctx[f].islfs() for f, n in files):
                        self.repo.requirements.add("lfs")
                        self.repo._writerequirements()

                        # Permanently enable lfs locally
                        with self.repo.localvfs("hgrc", "a", text=True) as fp:
                            fp.write("\n[extensions]\nlfs=\n")

                return node

        sink.__class__ = lfssink

    return sink


def vfsinit(orig, self, othervfs):
    orig(self, othervfs)
    # copy lfs related options
    for k, v in othervfs.options.items():
        if k.startswith("lfs"):
            self.options[k] = v
    # also copy lfs blobstores. note: this can run before reposetup, so lfs
    # blobstore attributes are not always ready at this time.
    for name in ["lfslocalblobstore", "lfsremoteblobstore"]:
        if util.safehasattr(othervfs, name):
            setattr(self, name, getattr(othervfs, name))


def hgclone(orig, ui, opts, *args, **kwargs):
    result = orig(ui, opts, *args, **kwargs)

    if result is not None:
        sourcerepo, destrepo = result
        repo = destrepo.local()

        # When cloning to a remote repo (like through SSH), no repo is available
        # from the peer.  Therefore the hgrc can't be updated.
        if not repo:
            return result

        # If lfs is required for this repo, permanently enable it locally
        if "lfs" in repo.requirements:
            with repo.localvfs("hgrc", "a", text=True) as fp:
                fp.write("\n[extensions]\nlfs=\n")

    return result


def hgpostshare(orig, sourcerepo, destrepo, **kwargs):
    orig(sourcerepo, destrepo, **kwargs)

    # If lfs is required for this repo, permanently enable it locally
    if "lfs" in destrepo.requirements:
        with destrepo.localvfs("hgrc", "a", text=True) as fp:
            fp.write("\n[extensions]\nlfs=\n")


def _canskipupload(repo):
    # if remotestore is a null store, upload is a no-op and can be skipped
    return isinstance(repo.svfs.lfsremoteblobstore, blobstore._nullremote)


def candownload(repo):
    # if remotestore is a null store, downloads will lead to nothing
    return not isinstance(repo.svfs.lfsremoteblobstore, blobstore._nullremote)


def uploadblobsfromrevs(repo, revs):
    """upload lfs blobs introduced by revs

    Note: also used by other extensions e. g. infinitepush. avoid renaming.
    """
    if _canskipupload(repo):
        return
    pointers = extractpointers(repo, revs)
    uploadblobs(repo, pointers)


def prepush(pushop):
    """Prepush hook.

    Read through the revisions to push, looking for filelog entries that can be
    deserialized into metadata so that we can block the push on their upload to
    the remote blobstore.
    """
    return uploadblobsfromrevs(pushop.repo, pushop.outgoing.missing)


def writenewbundle(
    orig, ui, repo, source, filename, bundletype, outgoing, *args, **kwargs
):
    """upload LFS blobs added by outgoing revisions on 'hg bundle'"""
    uploadblobsfromrevs(repo, outgoing.missing)
    return orig(ui, repo, source, filename, bundletype, outgoing, *args, **kwargs)


def _pushbundle2(orig, pushop):
    """upload LFS blobs added by outgoing revisions on 'hg push' using bundle2"""
    uploadblobsfromrevs(pushop.repo, pushop.outgoing.missing)
    return orig(pushop)


def checkrevs(orig, repo, revs):
    """verify LFS objects were uploaded when receiving pushes"""
    verify = repo.ui.config("lfs", "verify") or "none"
    if verify == "existance":
        pointers = extractpointers(repo, revs)
        if len(pointers) > 0:
            store = repo.svfs.lfsremoteblobstore
            store.checkblobs(pointers)
    elif verify == "none":
        pass
    else:
        raise error.Abort(_("invalid lfs.verify: %s") % verify)
    return orig(repo, revs)


def extractpointers(repo, revs):
    """return a list of lfs pointers added by given revs"""
    ui = repo.ui
    if ui.debugflag:
        ui.write(_("lfs: computing set of blobs to upload\n"))
    pointers = util.sortdict()
    for r in revs:
        ctx = repo[r]
        for p in pointersfromctx(ctx).values():
            pointers[p.oid()] = p
    return pointers.values()


def pointersfromctx(ctx):
    """return a dict {path: pointer} for given single changectx"""
    result = util.sortdict()
    for f in ctx.files():
        if f not in ctx:
            continue
        fctx = ctx[f]
        if not _islfs(fctx.filelog(), fctx.filenode()):
            continue
        try:
            result[f] = pointer.deserialize(fctx.rawdata())
        except pointer.InvalidPointer as ex:
            raise error.Abort(
                _("lfs: corrupted pointer (%s@%s): %s\n") % (f, short(ctx.node()), ex)
            )
    return result


def uploadblobs(repo, pointers):
    """upload given pointers from local blobstore"""
    if not pointers:
        return

    remoteblob = repo.svfs.lfsremoteblobstore
    remoteblob.writebatch(pointers, repo.svfs.lfslocalblobstore)


def upgradefinishdatamigration(orig, ui, srcrepo, dstrepo, requirements):
    orig(ui, srcrepo, dstrepo, requirements)

    srclfsvfs = srcrepo.svfs.lfslocalblobstore.vfs
    dstlfsvfs = dstrepo.svfs.lfslocalblobstore.vfs

    if srclfsvfs and dstlfsvfs:
        for dirpath, dirs, files in srclfsvfs.walk():
            for oid in files:
                ui.write(_("copying lfs blob %s\n") % oid)
                srclfsvfs.linktovfs(oid, dstlfsvfs)


def upgraderequirements(orig, repo):
    reqs = orig(repo)
    if "lfs" in repo.requirements:
        reqs.add("lfs")
    return reqs
