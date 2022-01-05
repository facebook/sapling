# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# placeholders.py - methods wrapping core mercurial logic when lfs.lfsplaceholders is set

from __future__ import absolute_import

from edenscm.mercurial import error, filelog, revlog
from edenscm.mercurial.i18n import _

from . import pointer, wrapper


def readfromstore(self, text):
    """Return placeholder instead of actual lfs file."""
    p = pointer.deserialize(text)
    isbinary = bool(int(p.get("x-is-binary", 1)))
    placeholder = (
        "This is a placeholder for a large file\n\n"
        "Original file id: %s\n"
        "Original file size: %s\n"
        "Original file is binary: %s\n" % (p["oid"], p.size(), isbinary)
    )

    # pack hg filelog metadata
    hgmeta = p.hgmeta()
    text = placeholder.encode("utf-8")
    if hgmeta:
        text = filelog.packmeta(hgmeta, text)
    return (text, False)


def filectxisbinary(orig, self):
    if self.islfs():
        """Placeholders are always text"""
        return False
    return orig(self)


def filectxcmp(orig, self, fctx):
    """returns True if text is different than fctx"""
    # some fctx (ex. hg-git) is not based on basefilectx and do not have islfs
    if self.islfs() and getattr(fctx, "islfs", lambda: False)():
        # fast path: check LFS oid
        p1 = pointer.deserialize(self.rawdata())
        p2 = pointer.deserialize(fctx.rawdata())
        return p1.oid() != p2.oid()

    if self.islfs() or getattr(fctx, "islfs", lambda: False)():
        # we can't rely on filelog hashing as the hashes don't match the reality
        return self.data() != fctx.data()

    return orig(self, fctx)


def filelogsize(orig, self, rev):
    if wrapper._islfs(self, rev=rev):
        rawtext = self.revision(rev, raw=True)
        placeholder = readfromstore(self, rawtext)
        return len(placeholder[0])
    return orig(self, rev)


def writetostore(self, text):
    raise error.Abort(_("can't write LFS files in placeholders mode"))


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
        raise error.Abort(_("can't write LFS files in placeholders mode"))

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
