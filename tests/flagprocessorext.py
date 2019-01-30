# coding=UTF-8

from __future__ import absolute_import

import base64
import zlib

from edenscm.mercurial import changegroup, exchange, extensions, filelog, revlog, util


# Test only: These flags are defined here only in the context of testing the
# behavior of the flag processor. The canonical way to add flags is to get in
# touch with the community and make them known in revlog.
REVIDX_NOOP = 1 << 3
REVIDX_BASE64 = 1 << 2
REVIDX_GZIP = 1 << 1
REVIDX_FAIL = 1


def validatehash(self, text):
    return True


def bypass(self, text):
    return False


def noopdonothing(self, text):
    return (text, True)


def b64encode(self, text):
    return (base64.b64encode(text), False)


def b64decode(self, text):
    return (base64.b64decode(text), True)


def gzipcompress(self, text):
    return (zlib.compress(text), False)


def gzipdecompress(self, text):
    return (zlib.decompress(text), True)


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


def noopaddrevision(
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
):
    if "[NOOP]" in text:
        flags |= REVIDX_NOOP
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
    )


def b64addrevision(
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
):
    if "[BASE64]" in text:
        flags |= REVIDX_BASE64
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
    )


def gzipaddrevision(
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
):
    if "[GZIP]" in text:
        flags |= REVIDX_GZIP
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
    )


def failaddrevision(
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
):
    # This addrevision wrapper is meant to add a flag we will not have
    # transforms registered for, ensuring we handle this error case.
    if "[FAIL]" in text:
        flags |= REVIDX_FAIL
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
    )


def extsetup(ui):
    # Enable changegroup3 for flags to be sent over the wire
    wrapfunction = extensions.wrapfunction
    wrapfunction(changegroup, "supportedoutgoingversions", supportedoutgoingversions)
    wrapfunction(changegroup, "allsupportedversions", allsupportedversions)

    # Teach revlog about our test flags
    flags = [REVIDX_NOOP, REVIDX_BASE64, REVIDX_GZIP, REVIDX_FAIL]
    revlog.REVIDX_KNOWN_FLAGS |= util.bitsfrom(flags)
    revlog.REVIDX_FLAGS_ORDER.extend(flags)

    # Teach exchange to use changegroup 3
    for k in exchange._bundlespeccgversions.keys():
        exchange._bundlespeccgversions[k] = "03"

    # Add wrappers for addrevision, responsible to set flags depending on the
    # revision data contents.
    wrapfunction(filelog.filelog, "addrevision", noopaddrevision)
    wrapfunction(filelog.filelog, "addrevision", b64addrevision)
    wrapfunction(filelog.filelog, "addrevision", gzipaddrevision)
    wrapfunction(filelog.filelog, "addrevision", failaddrevision)

    # Register flag processors for each extension
    revlog.addflagprocessor(REVIDX_NOOP, (noopdonothing, noopdonothing, validatehash))
    revlog.addflagprocessor(REVIDX_BASE64, (b64decode, b64encode, bypass))
    revlog.addflagprocessor(REVIDX_GZIP, (gzipdecompress, gzipcompress, bypass))
