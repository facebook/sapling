# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# bundle2.py - generic container format to transmit arbitrary data.
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Handling of the new bundle2 format

The goal of bundle2 is to act as an atomically packet to transmit a set of
payloads in an application agnostic way. It consist in a sequence of "parts"
that will be handed to and processed by the application layer.

General format architecture
===========================

The format is architectured as follow

 - magic string
 - stream level parameters
 - payload parts (any number)
 - end of stream marker.

the Binary format
============================

All numbers are unsigned and big-endian.

stream level parameters
------------------------

Binary format is as follow

:params size: int32

  The total number of Bytes used by the parameters

:params value: arbitrary number of Bytes

  A blob of `params size` containing the serialized version of all stream level
  parameters.

  The blob contains a space separated list of parameters. Parameters with value
  are stored in the form `<name>=<value>`. Both name and value are urlquoted.

  Empty name are obviously forbidden.

  Name MUST start with a letter. If this first letter is lower case, the
  parameter is advisory and can be safely ignored. However when the first
  letter is capital, the parameter is mandatory and the bundling process MUST
  stop if he is not able to proceed it.

  Stream parameters use a simple textual format for two main reasons:

  - Stream level parameters should remain simple and we want to discourage any
    crazy usage.
  - Textual data allow easy human inspection of a bundle2 header in case of
    troubles.

  Any Applicative level options MUST go into a bundle2 part instead.

Payload part
------------------------

Binary format is as follow

:header size: int32

  The total number of Bytes used by the part header. When the header is empty
  (size = 0) this is interpreted as the end of stream marker.

:header:

    The header defines how to interpret the part. It contains two piece of
    data: the part type, and the part parameters.

    The part type is used to route an application level handler, that can
    interpret payload.

    Part parameters are passed to the application level handler.  They are
    meant to convey information that will help the application level object to
    interpret the part payload.

    The binary format of the header is has follow

    :typesize: (one byte)

    :parttype: alphanumerical part name (restricted to [a-zA-Z0-9_:-]*)

    :partid: A 32bits integer (unique in the bundle) that can be used to refer
             to this part.

    :parameters:

        Part's parameter may have arbitrary content, the binary structure is::

            <mandatory-count><advisory-count><param-sizes><param-data>

        :mandatory-count: 1 byte, number of mandatory parameters

        :advisory-count:  1 byte, number of advisory parameters

        :param-sizes:

            N couple of bytes, where N is the total number of parameters. Each
            couple contains (<size-of-key>, <size-of-value) for one parameter.

        :param-data:

            A blob of bytes from which each parameter key and value can be
            retrieved using the list of size couples stored in the previous
            field.

            Mandatory parameters comes first, then the advisory ones.

            Each parameter's key MUST be unique within the part.

:payload:

    payload is a series of `<chunksize><chunkdata>`.

    `chunksize` is an int32, `chunkdata` are plain bytes (as much as
    `chunksize` says)` The payload part is concluded by a zero size chunk.

    The current implementation always produces either zero or one chunk.
    This is an implementation limitation that will ultimately be lifted.

    `chunksize` can be negative to trigger special case processing. No such
    processing is in place yet.

Bundle processing
============================

Each part is processed in order using a "part handler". Handler are registered
for a certain part type.

The matching of a part to its handler is case insensitive. The case of the
part type is used to know if a part is mandatory or advisory. If the Part type
contains any uppercase char it is considered mandatory. When no handler is
known for a Mandatory part, the process is aborted and an exception is raised.
If the part is advisory and no handler is known, the part is ignored. When the
process is aborted, the full bundle is still read from the stream to keep the
channel usable. But none of the part read from an abort are processed. In the
future, dropping the stream may become an option for channel we do not care to
preserve.
"""

from __future__ import absolute_import, division

import errno
import os
import re
import string
import struct
import sys
from typing import Any, Callable, Dict, Iterable, List, Optional, Tuple

from . import (
    bookmarks,
    changegroup,
    discovery,
    error,
    node as nodemod,
    obsolete,
    perftrace,
    phases,
    pushkey,
    pycompat,
    url,
    urllibcompat,
    util,
)
from .i18n import _
from .vfs import abstractvfs


_pack = struct.pack
_unpack = struct.unpack

_fstreamparamsize = ">i"
_fpartheadersize = ">i"
_fparttypesize = ">B"
_fpartid = ">I"
_fpayloadsize = ">i"
_fpartparamcount = ">BB"

preferedchunksize = 4096

_parttypeforbidden = re.compile("[^a-zA-Z0-9_:-]")


def outdebug(ui, message):
    """debug regarding output stream (bundling)"""
    if ui.configbool("devel", "bundle2.debug"):
        ui.debug("bundle2-output: %s\n" % message)


def indebug(ui, message):
    """debug on input stream (unbundling)"""
    if ui.configbool("devel", "bundle2.debug"):
        ui.debug("bundle2-input: %s\n" % message)


def validateparttype(parttype):
    """raise ValueError if a parttype contains invalid character"""
    if _parttypeforbidden.search(parttype):
        raise ValueError(parttype)


def _makefpartparamsizes(nbparams):
    """return a struct format to read part parameter sizes

    The number parameters is variable so we need to build that format
    dynamically.
    """
    return ">" + ("BB" * nbparams)


parthandlermapping = {}


def parthandler(
    parttype: str,
    params: "Tuple[str, ...]" = ()
    # pyre-fixme[31]: Expression `unbundlepart), None)])]` is not a valid type.
) -> "Callable[Callable[(bundleoperation, unbundlepart), None], Callable[(bundleoperation, unbundlepart), None]]":
    """decorator that register a function as a bundle2 part handler

    eg::

        @parthandler('myparttype', ('mandatory', 'param', 'handled'))
        def myparttypehandler(...):
            '''process a part of type "my part".'''
            ...
    """
    validateparttype(parttype)

    def _decorator(func):
        lparttype = parttype.lower()  # enforce lower case matching.
        assert lparttype not in parthandlermapping
        parthandlermapping[lparttype] = func
        func.params = frozenset(params)
        return func

    return _decorator


class unbundlerecords(object):
    """keep record of what happens during and unbundle

    New records are added using `records.add('cat', obj)`. Where 'cat' is a
    category of record and obj is an arbitrary object.

    `records['cat']` will return all entries of this category 'cat'.

    Iterating on the object itself will yield `('category', obj)` tuples
    for all entries.

    All iterations happens in chronological order.
    """

    def __init__(self):
        self._categories = {}
        self._sequences = []
        self._replies = {}

    def add(self, category, entry, inreplyto=None):
        """add a new record of a given category.

        The entry can then be retrieved in the list returned by
        self['category']."""
        self._categories.setdefault(category, []).append(entry)
        self._sequences.append((category, entry))
        if inreplyto is not None:
            self.getreplies(inreplyto).add(category, entry)

    def getreplies(self, partid):
        """get the records that are replies to a specific part"""
        return self._replies.setdefault(partid, unbundlerecords())

    def __getitem__(self, cat):
        return tuple(self._categories.get(cat, ()))

    def __iter__(self):
        return iter(self._sequences)

    def __len__(self):
        return len(self._sequences)

    def __nonzero__(self):
        return bool(self._sequences)

    __bool__ = __nonzero__


class bundleoperation(object):
    """an object that represents a single bundling process

    Its purpose is to carry unbundle-related objects and states.

    A new object should be created at the beginning of each bundle processing.
    The object is to be returned by the processing function.

    The object has very little content now it will ultimately contain:
    * an access to the repo the bundle is applied to,
    * a ui object,
    * a way to retrieve a transaction to add changes to the repo,
    * a way to record the result of processing each part,
    * a way to construct a bundle response when applicable.
    * an indication whether to minimize the response size (`respondlightly`)

    Note that `respondlightly` exists independently of `reply.capabilities`.
    The latter is built form the `replycaps` bundle2 part and in case of
    `unbundlereplay` we don't construct the bundle2, but rather just replay
    the saved one, so we can't change this part. Thus the need for another
    way of indicating the need to mininize response.
    """

    def __init__(
        self,
        repo,
        transactiongetter,
        captureoutput=True,
        replaydata=None,
        respondlightly=False,
        extras=None,
    ):
        self.repo = repo
        self.ui = repo.ui
        self.records = unbundlerecords()
        self.reply: "Optional[bundle20]" = None
        self.captureoutput = captureoutput
        self.hookargs = {}
        self._gettransaction = transactiongetter
        # carries value that can modify part behavior
        self.modes = {}
        # whether to produce response parts
        self.respondlightly = respondlightly
        self.replaydata = replaydata
        self._addreplayhookargs()
        self.extras = extras or {}

    def _addreplayhookargs(self):
        if self.replaydata is None:
            return
        if self.replaydata.rebasedhead is not None:
            self.hookargs["EXPECTED_REBASEDHEAD"] = self.replaydata.rebasedhead
        if self.replaydata.ontobook is not None:
            self.hookargs["EXPECTED_ONTOBOOK"] = self.replaydata.ontobook

        self.hookargs["IS_UNBUNDLE_REPLAY"] = "true"

    def gettransaction(self):
        transaction = self._gettransaction()

        if self.hookargs:
            # the ones added to the transaction supercede those added
            # to the operation.
            self.hookargs.update(transaction.hookargs)
            transaction.hookargs = self.hookargs

        # mark the hookargs as flushed.  further attempts to add to
        # hookargs will result in an abort.
        self.hookargs = None

        return transaction

    def addhookargs(self, hookargs):
        if self.hookargs is None:
            raise error.ProgrammingError(
                "attempted to add hookargs to " "operation after transaction started"
            )
        self.hookargs.update(hookargs)


class TransactionUnavailable(RuntimeError):
    pass


def _notransaction():
    """default method to get a transaction while processing a bundle

    Raise an exception to highlight the fact that no transaction was expected
    to be created"""
    raise TransactionUnavailable()


def applybundle(repo, unbundler, tr, source=None, url=None, **kwargs):
    # transform me into unbundler.apply() as soon as the freeze is lifted
    if isinstance(unbundler, unbundle20):
        tr.hookargs["bundle2"] = "1"
        if source is not None and "source" not in tr.hookargs:
            tr.hookargs["source"] = source
        if url is not None and "url" not in tr.hookargs:
            tr.hookargs["url"] = url
        return processbundle(repo, unbundler, lambda: tr)
    else:
        # the transactiongetter won't be used, but we might as well set it
        op = bundleoperation(repo, lambda: tr)
        _processchangegroup(op, unbundler, tr, source, url, **kwargs)
        return op


class partiterator(object):
    def __init__(self, repo, op, unbundler):
        self.repo = repo
        self.op = op
        self.unbundler = unbundler
        self.iterator = None
        self.count = 0
        self.current = None

    def __enter__(self):
        def func():
            itr = enumerate(self.unbundler.iterparts())
            for count, p in itr:
                self.count = count
                self.current = p
                yield p
                p.consume()
                self.current = None

        self.iterator = func()
        return self.iterator

    def __exit__(self, type, exc, tb):
        if not self.iterator:
            return

        # Only gracefully abort in a normal exception situation. User aborts
        # like Ctrl+C throw a KeyboardInterrupt which is not a base Exception,
        # and should not gracefully cleanup.
        if isinstance(exc, Exception):
            # Any exceptions seeking to the end of the bundle at this point are
            # almost certainly related to the underlying stream being bad.
            # And, chances are that the exception we're handling is related to
            # getting in that bad state. So, we swallow the seeking error and
            # re-raise the original error.
            seekerror = False
            try:
                if self.current:
                    # consume the part content to not corrupt the stream.
                    self.current.consume()

                for part in self.iterator:
                    # consume the bundle content
                    part.consume()
            except Exception:
                seekerror = True

            # Small hack to let caller code distinguish exceptions from bundle2
            # processing from processing the old format. This is mostly needed
            # to handle different return codes to unbundle according to the type
            # of bundle. We should probably clean up or drop this return code
            # craziness in a future version.
            exc.duringunbundle2 = True
            salvaged = []
            replycaps = None
            if self.op is not None and self.op.reply is not None:
                salvaged = self.op.reply.salvageoutput()
                replycaps = self.op.reply.capabilities
            exc._replycaps = replycaps
            exc._bundle2salvagedoutput = salvaged

            # Re-raising from a variable loses the original stack. So only use
            # that form if we need to.
            if seekerror:
                raise exc

        self.repo.ui.debug("bundle2-input-bundle: %i parts total\n" % self.count)


def processbundle(repo, unbundler, transactiongetter=None, op=None):
    """This function process a bundle, apply effect to/from a repo

    It iterates over each part then searches for and uses the proper handling
    code to process the part. Parts are processed in order.

    Unknown Mandatory part will abort the process.

    It is temporarily possible to provide a prebuilt bundleoperation to the
    function. This is used to ensure output is properly propagated in case of
    an error during the unbundling. This output capturing part will likely be
    reworked and this ability will probably go away in the process.
    """
    if op is None:
        if transactiongetter is None:
            transactiongetter = _notransaction
        op = bundleoperation(repo, transactiongetter)
    # todo:
    # - replace this is a init function soon.
    # - exception catching
    unbundler.params
    if repo.ui.debugflag:
        msg = ["bundle2-input-bundle:"]
        if unbundler.params:
            msg.append(" %i params" % len(unbundler.params))
        if op._gettransaction is None or op._gettransaction is _notransaction:
            msg.append(" no-transaction")
        else:
            msg.append(" with-transaction")
        msg.append("\n")
        repo.ui.debug("".join(msg))

    processparts(repo, op, unbundler)

    return op


def processparts(repo, op, unbundler):
    with partiterator(repo, op, unbundler) as parts:
        for part in parts:
            _processpart(op, part)


@perftrace.tracefunc("Apply Changegroup")
def _processchangegroup(op, cg, tr, source, url, **kwargs):
    kwargs["updatevisibility"] = op.extras.get("updatevisibility", True)
    ret = cg.apply(op.repo, tr, source, url, **kwargs)
    op.records.add("changegroup", {"return": ret})
    return ret


def _gethandler(op, part):
    status = "unknown"  # used by debug output
    try:
        handler = parthandlermapping.get(part.type)
        if handler is None:
            status = "unsupported-type"
            raise error.BundleUnknownFeatureError(parttype=part.type)
        indebug(op.ui, "found a handler for part %s" % part.type)
        unknownparams = part.mandatorykeys - handler.params
        if unknownparams:
            unknownparams = list(unknownparams)
            unknownparams.sort()
            status = "unsupported-params (%s)" % ", ".join(unknownparams)
            raise error.BundleUnknownFeatureError(
                parttype=part.type, params=unknownparams
            )
        status = "supported"
    except error.BundleUnknownFeatureError as exc:
        if part.mandatory:  # mandatory parts
            raise
        indebug(op.ui, "ignoring unsupported advisory part %s" % exc)
        return  # skip to part processing
    finally:
        if op.ui.debugflag:
            msg = ['bundle2-input-part: "%s"' % part.type]
            if not part.mandatory:
                msg.append(" (advisory)")
            nbmp = len(part.mandatorykeys)
            nbap = len(part.params) - nbmp
            if nbmp or nbap:
                msg.append(" (params:")
                if nbmp:
                    msg.append(" %i mandatory" % nbmp)
                if nbap:
                    msg.append(" %i advisory" % nbap)
                msg.append(")")
            msg.append(" %s\n" % status)
            op.ui.debug("".join(msg))

    return handler


def _processpart(op, part):
    """process a single part from a bundle

    The part is guaranteed to have been fully consumed when the function exits
    (even if an exception is raised)."""
    handler = _gethandler(op, part)
    if handler is None:
        return

    # handler is called outside the above try block so that we don't
    # risk catching KeyErrors from anything other than the
    # parthandlermapping lookup (any KeyError raised by handler()
    # itself represents a defect of a different variety).
    output = None
    if op.captureoutput and op.reply is not None:
        op.ui.pushbuffer(error=True, subproc=True)
        output = b""
    try:
        handler(op, part)
    finally:
        if output is not None:
            output = b"".join(
                buf if isinstance(buf, bytes) else pycompat.encodeutf8(buf)
                for buf in op.ui.popbufferlist()
            )

        if output:
            outpart = op.reply.newpart("output", data=output, mandatory=False)
            outpart.addparam("in-reply-to", pycompat.bytestr(part.id), mandatory=False)


def decodecaps(blob: str) -> "Dict[str, Tuple[str, ...]]":
    """decode a bundle2 caps string blob into a dictionary

    The blob is a list of capabilities (one per line)
    Capabilities may have values using a line of the form::

        capability=value1,value2,value3

    The values are always a list."""
    caps = {}
    for line in blob.splitlines():
        if not line:
            continue
        if "=" not in line:
            key, vals = line, list()
        else:
            key, vals = line.split("=", 1)
            vals = vals.split(",")
        key = urllibcompat.unquote(key)
        vals = [urllibcompat.unquote(v) for v in vals]
        caps[key] = tuple(vals)
    return caps


def encodecaps(caps):
    # type (Dict[str, List[str]]) -> str
    """encode a bundle2 caps dictionary into a string blob"""
    chunks = []
    for ca in sorted(caps):
        vals = caps[ca]
        ca = urllibcompat.quote(ca)
        vals = [urllibcompat.quote(v) for v in vals]
        if vals:
            ca = "%s=%s" % (ca, ",".join(vals))
        chunks.append(ca)
    return "\n".join(chunks)


bundletypes = {
    "": (b"", "UN"),  # only when using unbundle on ssh and old http servers
    # since the unification ssh accepts a header but there
    # is no capability signaling it.
    "HG20": (),  # special-cased below
    "HG10UN": (b"HG10UN", "UN"),
    "HG10BZ": (b"HG10", "BZ"),
    "HG10GZ": (b"HG10GZ", "GZ"),
}

# hgweb uses this list to communicate its preferred type
bundlepriority = ["HG10GZ", "HG10BZ", "HG10UN"]


class bundle20(object):
    """represent an outgoing bundle2 container

    Use the `addparam` method to add stream level parameter. and `newpart` to
    populate it. Then call `getchunks` to retrieve all the binary chunks of
    data that compose the bundle2 container."""

    _magicstring = b"HG20"

    def __init__(self, ui, capabilities=()):
        self.ui = ui
        self._params: "List[str, str]" = []
        self._parts = []
        self.capabilities = dict(capabilities)
        self._compengine = util.compengines.forbundletype("UN")
        self._compopts = None

    def setcompression(
        self, alg: "Optional[str]", compopts: "Optional[Dict]" = None
    ) -> None:
        """setup core part compression to <alg>"""
        if alg in (None, "UN"):
            return
        assert not any(n.lower() == "compression" for n, v in self._params)
        self.addparam("Compression", alg)
        self._compengine = util.compengines.forbundletype(alg)
        self._compopts = compopts

    @property
    def nbparts(self):
        """total number of parts added to the bundler"""
        return len(self._parts)

    # methods used to defines the bundle2 content
    def addparam(self, name: str, value: "Optional[str]" = None) -> None:
        """add a stream level parameter"""
        if not name:
            raise ValueError(r"empty parameter name")
        if name[0:1] not in pycompat.bytestr(string.ascii_letters):
            raise ValueError(r"non letter first character: %s" % name)
        self._params.append((name, value))

    def addpart(self, part: "bundlepart") -> None:
        """add a new part to the bundle2 container

        Parts contains the actual applicative payload."""
        assert part.id is None
        part.id = len(self._parts)  # very cheap counter
        self._parts.append(part)

    def newpart(self, typeid, *args, **kwargs):
        """create a new part and add it to the containers

        As the part is directly added to the containers. For now, this means
        that any failure to properly initialize the part after calling
        ``newpart`` should result in a failure of the whole bundling process.

        You can still fall back to manually create and add if you need better
        control."""
        part = bundlepart(typeid, *args, **kwargs)
        self.addpart(part)
        return part

    # methods used to generate the bundle2 stream
    def getchunks(self) -> "Iterable[bytes]":
        if self.ui.debugflag:
            msg = [
                'bundle2-output-bundle: "%s",' % pycompat.decodeutf8(self._magicstring)
            ]
            if self._params:
                msg.append(" (%i params)" % len(self._params))
            msg.append(" %i parts total\n" % len(self._parts))
            self.ui.debug("".join(msg))
        outdebug(
            self.ui,
            "start emission of %s stream" % pycompat.decodeutf8(self._magicstring),
        )
        yield self._magicstring
        param = self._paramchunk()
        outdebug(self.ui, "bundle parameter: %s" % pycompat.decodeutf8(param))
        yield _pack(_fstreamparamsize, len(param))
        if param:
            yield param
        for chunk in self._compengine.compressstream(
            self._getcorechunk(), self._compopts
        ):
            yield chunk

    def _paramchunk(self) -> bytes:
        """return a encoded version of all stream parameters"""
        blocks = []
        for par, value in self._params:
            par = urllibcompat.quote(par)
            if value is not None:
                value = urllibcompat.quote(value)
                par = "%s=%s" % (par, value)
            blocks.append(par)
        return pycompat.encodeutf8(" ".join(blocks))

    def _getcorechunk(self) -> "Iterable[bytes]":
        """yield chunk for the core part of the bundle

        (all but headers and parameters)"""
        outdebug(self.ui, "start of parts")
        for part in self._parts:
            outdebug(self.ui, 'bundle part: "%s"' % part.type)
            for chunk in part.getchunks(ui=self.ui):
                yield chunk
        outdebug(self.ui, "end of bundle")
        yield _pack(_fpartheadersize, 0)

    def salvageoutput(self):
        """return a list with a copy of all output parts in the bundle

        This is meant to be used during error handling to make sure we preserve
        server output"""
        salvaged = []
        for part in self._parts:
            if part.type.startswith("output"):
                salvaged.append(part.copy())
        return salvaged


class unpackermixin(object):
    """A mixin to extract bytes and struct data from a stream"""

    def __init__(self, fp):
        self._fp = fp

    def _unpack(self, format):
        """unpack this struct format from the stream

        This method is meant for internal usage by the bundle2 protocol only.
        They directly manipulate the low level stream including bundle2 level
        instruction.

        Do not use it to implement higher-level logic or methods."""
        data = self._readexact(struct.calcsize(format))
        return _unpack(format, data)

    def _readexact(self, size):
        """read exactly <size> bytes from the stream

        This method is meant for internal usage by the bundle2 protocol only.
        They directly manipulate the low level stream including bundle2 level
        instruction.

        Do not use it to implement higher-level logic or methods."""
        return changegroup.readexactly(self._fp, size)


def getunbundler(ui, fp, magicstring=None):
    """return a valid unbundler object for a given magicstring"""
    if magicstring is None:
        magicstring = pycompat.decodeutf8(changegroup.readexactly(fp, 4))
    magic, version = magicstring[0:2], magicstring[2:4]
    if magic != "HG":
        ui.debug(
            "error: invalid magic: %r (version %r), should be 'HG'\n" % (magic, version)
        )
        raise error.Abort(_("not a Mercurial bundle"))
    unbundlerclass = formatmap.get(version)
    if unbundlerclass is None:
        raise error.Abort(_("unknown bundle version %s") % version)
    unbundler = unbundlerclass(ui, fp)
    indebug(ui, "start processing of %s stream" % magicstring)
    return unbundler


b2streamparamsmap = {}


def b2streamparamhandler(name):
    """register a handler for a stream level parameter"""

    def decorator(func):
        assert name not in formatmap
        b2streamparamsmap[name] = func
        return func

    return decorator


class unbundle20(unpackermixin):
    """interpret a bundle2 stream

    This class is fed with a binary stream and yields parts through its
    `iterparts` methods."""

    _magicstring = b"HG20"

    def __init__(self, ui, fp):
        """If header is specified, we do not read it out of the stream."""
        self.ui = ui
        self._compengine = util.compengines.forbundletype("UN")
        self._compressed = None
        super(unbundle20, self).__init__(fp)

    @util.propertycache
    def params(self):
        """dictionary of stream level parameters"""
        indebug(self.ui, "reading bundle2 stream parameters")
        params = {}
        paramssize = self._unpack(_fstreamparamsize)[0]
        if paramssize < 0:
            raise error.BundleValueError("negative bundle param size: %i" % paramssize)
        if paramssize:
            params = self._readexact(paramssize)
            params = self._processallparams(params)
        return params

    def _processallparams(self, paramsblock: bytes) -> "Dict[str, Optional[str]]":
        """ """
        params = util.sortdict()
        data = pycompat.decodeutf8(paramsblock)
        for param in data.split(" "):
            p = param.split("=", 1)
            p = [urllibcompat.unquote(i) for i in p]
            assert len(p) >= 1
            if len(p) == 1:
                self._processparam(p[0], None)
                params[p[0]] = None
            else:
                self._processparam(p[0], p[1])
                params[p[0]] = p[1]
        return params

    def _processparam(self, name: str, value: "Optional[str]") -> None:
        """process a parameter, applying its effect if needed

        Parameter starting with a lower case letter are advisory and will be
        ignored when unknown.  Those starting with an upper case letter are
        mandatory and will this function will raise a KeyError when unknown.

        Note: no option are currently supported. Any input will be either
              ignored or failing.
        """
        if not name:
            raise ValueError(r"empty parameter name")
        if name[0:1] not in pycompat.bytestr(string.ascii_letters):
            raise ValueError(r"non letter first character: %s" % name)
        try:
            handler = b2streamparamsmap[name.lower()]
        except KeyError:
            if name[0:1].islower():
                indebug(self.ui, "ignoring unknown parameter %s" % name)
            else:
                raise error.BundleUnknownFeatureError(params=(name,))
        else:
            handler(self, name, value)

    def _forwardchunks(self):
        """utility to transfer a bundle2 as binary

        This is made necessary by the fact the 'getbundle' command over 'ssh'
        have no way to know then the reply end, relying on the bundle to be
        interpreted to know its end. This is terrible and we are sorry, but we
        needed to move forward to get general delta enabled.
        """
        yield self._magicstring
        assert "params" not in vars(self)
        paramssize = self._unpack(_fstreamparamsize)[0]
        if paramssize < 0:
            raise error.BundleValueError("negative bundle param size: %i" % paramssize)
        yield _pack(_fstreamparamsize, paramssize)
        if paramssize:
            params = self._readexact(paramssize)
            self._processallparams(params)
            yield params
            assert self._compengine.bundletype == "UN"
        # From there, payload might need to be decompressed
        self._fp = self._compengine.decompressorreader(self._fp)
        emptycount = 0
        while emptycount < 2:
            # so we can brainlessly loop
            assert _fpartheadersize == _fpayloadsize
            size = self._unpack(_fpartheadersize)[0]
            yield _pack(_fpartheadersize, size)
            if size:
                emptycount = 0
            else:
                emptycount += 1
                continue
            if size == flaginterrupt:
                continue
            elif size < 0:
                raise error.BundleValueError("negative chunk size: %i")
            yield self._readexact(size)

    def iterparts(self, seekable=False):
        """yield all parts contained in the stream"""
        cls = seekableunbundlepart if seekable else unbundlepart
        # make sure param have been loaded
        self.params
        # From there, payload need to be decompressed
        self._fp = self._compengine.decompressorreader(self._fp)
        indebug(self.ui, "start extraction of bundle2 parts")
        headerblock = self._readpartheader()
        while headerblock is not None:
            part = cls(self.ui, headerblock, self._fp)
            yield part
            # Ensure part is fully consumed so we can start reading the next
            # part.
            part.consume()

            headerblock = self._readpartheader()
        indebug(self.ui, "end of bundle2 stream")

    def _readpartheader(self) -> "Optional[bytes]":
        """reads a part header size and return the bytes blob

        returns None if empty"""
        headersize = self._unpack(_fpartheadersize)[0]
        if headersize < 0:
            raise error.BundleValueError("negative part header size: %i" % headersize)
        indebug(self.ui, "part header size: %i" % headersize)
        if headersize:
            return self._readexact(headersize)
        return None

    def compressed(self) -> "Optional[bool]":
        self.params  # load params
        return self._compressed

    def close(self) -> None:
        """close underlying file"""
        if util.safehasattr(self._fp, "close"):
            return self._fp.close()


formatmap = {"20": unbundle20}


@b2streamparamhandler("compression")
def processcompression(unbundler: "unbundle20", param: str, value: str) -> None:
    """read compression parameter and install payload decompression"""
    if value not in util.compengines.supportedbundletypes:
        raise error.BundleUnknownFeatureError(params=(param,), values=(value,))
    unbundler._compengine = util.compengines.forbundletype(value)
    if value is not None:
        unbundler._compressed = True


class bundlepart(object):
    """A bundle2 part contains application level payload

    The part `type` is used to route the part to the application level
    handler.

    The part payload is contained in ``part.data``. It could be raw bytes or a
    generator of byte chunks.

    You can add parameters to the part using the ``addparam`` method.
    Parameters can be either mandatory (default) or advisory. Remote side
    should be able to safely ignore the advisory ones.

    Both data and parameters cannot be modified after the generation has begun.
    """

    def __init__(
        self,
        parttype: str,
        mandatoryparams: "Iterable[Tuple[str, str]]" = (),
        advisoryparams: "Iterable[Tuple[str, str]]" = (),
        data: bytes = b"",
        mandatory: bool = True,
    ) -> None:
        with perftrace.trace("Create bundle part"):
            validateparttype(parttype)
            self.id = None
            self.type = parttype
            assert (
                isinstance(data, bytes)
                or util.safehasattr(data, "next")
                or util.safehasattr(data, "__next__")
            )

            self._data = data
            self._mandatoryparams: "List[Tuple[str, str]]" = list(mandatoryparams)
            self._advisoryparams: "List[Tuple[str, str]]" = list(advisoryparams)
            # checking for duplicated entries
            self._seenparams = set()
            for pname, __ in self._mandatoryparams + self._advisoryparams:
                if pname in self._seenparams:
                    raise error.ProgrammingError("duplicated params: %s" % pname)
                self._seenparams.add(pname)
            # status of the part's generation:
            # - None: not started,
            # - False: currently generated,
            # - True: generation done.
            self._generated = None
            self.mandatory = mandatory

            perftrace.tracevalue("part type", parttype)
            if isinstance(data, bytes):
                perftrace.tracebytes("part data size", len(data))

    def __repr__(self):
        cls = "%s.%s" % (self.__class__.__module__, self.__class__.__name__)
        return "<%s object at %x; id: %s; type: %s; mandatory: %s>" % (
            cls,
            id(self),
            self.id,
            self.type,
            self.mandatory,
        )

    def copy(self):
        """return a copy of the part

        The new part have the very same content but no partid assigned yet.
        Parts with generated data cannot be copied."""
        assert not util.safehasattr(self.data, "next")
        return self.__class__(
            self.type,
            self._mandatoryparams,
            self._advisoryparams,
            self._data,
            self.mandatory,
        )

    # methods used to defines the part content
    @property
    def data(self) -> bytes:
        return self._data

    @data.setter
    def data(self, data):
        if self._generated is not None:
            raise error.ReadOnlyPartError("part is being generated")
        if not (
            util.safehasattr(self.data, "next")
            or util.safehasattr(self.data, "__next__")
        ):
            assert isinstance(data, bytes)
        self._data = data

    @property
    def mandatoryparams(self) -> "Tuple[Tuple[str, str], ...]":
        # make it an immutable tuple to force people through ``addparam``
        return tuple(self._mandatoryparams)

    @property
    def advisoryparams(self) -> "Tuple[Tuple[str, str], ...]":
        # make it an immutable tuple to force people through ``addparam``
        return tuple(self._advisoryparams)

    def addparam(self, name: str, value: str = "", mandatory: bool = True) -> None:
        """add a parameter to the part

        If 'mandatory' is set to True, the remote handler must claim support
        for this parameter or the unbundling will be aborted.

        The 'name' and 'value' cannot exceed 255 bytes each.
        """
        if self._generated is not None:
            raise error.ReadOnlyPartError("part is being generated")
        if name in self._seenparams:
            raise ValueError("duplicated params: %s" % name)
        self._seenparams.add(name)
        params = self._advisoryparams
        if mandatory:
            params = self._mandatoryparams
        params.append((name, value))

    # methods used to generates the bundle2 stream
    def getchunks(self, ui: "Any") -> "Iterable[bytes]":
        if self._generated is not None:
            raise error.ProgrammingError("part can only be consumed once")
        self._generated = False

        if ui.debugflag:
            msg = ['bundle2-output-part: "%s"' % self.type]
            if not self.mandatory:
                msg.append(" (advisory)")
            nbmp = len(self.mandatoryparams)
            nbap = len(self.advisoryparams)
            if nbmp or nbap:
                msg.append(" (params:")
                if nbmp:
                    msg.append(" %i mandatory" % nbmp)
                if nbap:
                    msg.append(" %i advisory" % nbap)
                msg.append(")")
            if not self.data:
                msg.append(" empty payload")
            elif util.safehasattr(self.data, "next") or util.safehasattr(
                self.data, "__next__"
            ):
                msg.append(" streamed payload")
            else:
                msg.append(" %i bytes payload" % len(self.data))
            msg.append("\n")
            ui.debug("".join(msg))

        #### header
        if self.mandatory:
            parttype = self.type.upper()
        else:
            parttype = self.type.lower()
        outdebug(ui, 'part %s: "%s"' % (pycompat.bytestr(self.id), parttype))
        ## parttype
        header = [
            _pack(_fparttypesize, len(parttype)),
            pycompat.encodeutf8(parttype),
            _pack(_fpartid, self.id),
        ]
        ## parameters
        # count
        manpar = self.mandatoryparams
        advpar = self.advisoryparams
        header.append(_pack(_fpartparamcount, len(manpar), len(advpar)))
        # size
        parsizes = []
        for key, value in manpar:
            parsizes.append(len(key))
            parsizes.append(len(value))
        for key, value in advpar:
            parsizes.append(len(key))
            parsizes.append(len(value))
        paramsizes = _pack(_makefpartparamsizes(len(parsizes) // 2), *parsizes)
        header.append(paramsizes)
        # key, value
        for key, value in manpar:
            header.append(pycompat.encodeutf8(key))
            header.append(pycompat.encodeutf8(value))
        for key, value in advpar:
            header.append(pycompat.encodeutf8(key))
            header.append(pycompat.encodeutf8(value))
        ## finalize header
        try:
            headerchunk = b"".join(header)
        except TypeError:
            raise TypeError(
                r"Found a non-bytes trying to build bundle part header: %r" % header
            )
        outdebug(ui, "header chunk size: %i" % len(headerchunk))
        yield _pack(_fpartheadersize, len(headerchunk))
        yield headerchunk
        ## payload
        try:
            for chunk in self._payloadchunks(ui):
                outdebug(ui, "payload chunk size: %i" % len(chunk))
                yield _pack(_fpayloadsize, len(chunk))
                yield chunk
        except GeneratorExit:
            # GeneratorExit means that nobody is listening for our
            # results anyway, so just bail quickly rather than trying
            # to produce an error part.
            ui.debug("bundle2-generatorexit\n")
            raise
        except BaseException as exc:
            bexc = util.forcebytestr(exc)
            # backup exception data for later
            ui.debug("bundle2-input-stream-interrupt: encoding exception %s" % bexc)
            tb = sys.exc_info()[2]
            msg = "unexpected error: %s" % bexc
            interpart = createerrorpart(msg, mandatory=False)
            interpart.id = 0
            yield _pack(_fpayloadsize, -1)
            for chunk in interpart.getchunks(ui=ui):
                yield chunk
            outdebug(ui, "closing payload chunk")
            # abort current part payload
            yield _pack(_fpayloadsize, 0)
            pycompat.raisewithtb(exc, tb)
        # end of payload
        outdebug(ui, "closing payload chunk")
        yield _pack(_fpayloadsize, 0)
        self._generated = True

    def _payloadchunks(self, ui):
        """yield chunks of a the part payload

        Exists to handle the different methods to provide data to a part."""
        data = self.data

        # If data is a large blob, let's convert it to chunks so the chunkbuffer
        # below will split it into smaller chunks.
        chunkthreshold = ui.configbytes("bundle2", "rechunkthreshold")
        if isinstance(data, bytes) and len(data) > chunkthreshold:
            data = iter([self.data])

        if util.safehasattr(data, "next") or util.safehasattr(data, "__next__"):
            buff = util.chunkbuffer(data)
            chunk = buff.read(preferedchunksize)
            while chunk:
                assert isinstance(chunk, bytes)
                yield chunk
                chunk = buff.read(preferedchunksize)
        elif len(data):
            assert isinstance(data, bytes)
            yield data


flaginterrupt = -1


class interrupthandler(unpackermixin):
    """read one part and process it with restricted capability

    This allows to transmit exception raised on the producer size during part
    iteration while the consumer is reading a part.

    Part processed in this manner only have access to a ui object,"""

    def __init__(self, ui, fp):
        super(interrupthandler, self).__init__(fp)
        self.ui = ui

    def _readpartheader(self) -> "Optional[bytes]":
        """reads a part header size and return the bytes blob

        returns None if empty"""
        headersize = self._unpack(_fpartheadersize)[0]
        if headersize < 0:
            raise error.BundleValueError("negative part header size: %i" % headersize)
        indebug(self.ui, "part header size: %i\n" % headersize)
        if headersize:
            return self._readexact(headersize)
        return None

    def __call__(self):

        self.ui.debug(
            "bundle2-input-stream-interrupt:" " opening out of band context\n"
        )
        indebug(self.ui, "bundle2 stream interruption, looking for a part.")
        headerblock = self._readpartheader()
        if headerblock is None:
            indebug(self.ui, "no part found during interruption.")
            return
        part = unbundlepart(self.ui, headerblock, self._fp)
        op = interruptoperation(self.ui)
        hardabort = False
        try:
            _processpart(op, part)
        except (SystemExit, KeyboardInterrupt):
            hardabort = True
            raise
        finally:
            if not hardabort:
                part.consume()
        self.ui.debug(
            "bundle2-input-stream-interrupt:" " closing out of band context\n"
        )


class interruptoperation(object):
    """A limited operation to be use by part handler during interruption

    It only have access to an ui object.
    """

    def __init__(self, ui):
        self.ui = ui
        self.reply = None
        self.captureoutput = False

    @property
    def repo(self):
        raise error.ProgrammingError("no repo access from stream interruption")

    def gettransaction(self):
        raise TransactionUnavailable("no repo access from stream interruption")


def decodepayloadchunks(ui: "Any", fh: "Any") -> "Iterable[bytes]":
    """Reads bundle2 part payload data into chunks.

    Part payload data consists of framed chunks. This function takes
    a file handle and emits those chunks.
    """
    dolog = ui.configbool("devel", "bundle2.debug")
    debug = ui.debug

    headerstruct = struct.Struct(_fpayloadsize)
    headersize = headerstruct.size
    unpack = headerstruct.unpack

    readexactly = changegroup.readexactly
    read = fh.read

    chunksize = unpack(readexactly(fh, headersize))[0]
    indebug(ui, "payload chunk size: %i" % chunksize)

    # changegroup.readexactly() is inlined below for performance.
    while chunksize:
        if chunksize >= 0:
            s = read(chunksize)
            if len(s) < chunksize:
                raise error.NetworkError.fewerbytesthanexpected(chunksize, len(s))

            yield s
        elif chunksize == flaginterrupt:
            # Interrupt "signal" detected. The regular stream is interrupted
            # and a bundle2 part follows. Consume it.
            interrupthandler(ui, fh)()
        else:
            raise error.BundleValueError("negative payload chunk size: %s" % chunksize)

        s = read(headersize)
        if len(s) < headersize:
            raise error.NetworkError.fewerbytesthanexpected(headersize, len(s))

        chunksize = unpack(s)[0]

        # indebug() inlined for performance.
        if dolog:
            debug("bundle2-input: payload chunk size: %i\n" % chunksize)


class unbundlepart(unpackermixin):
    """a bundle part read from a bundle"""

    def __init__(self, ui, header, fp):
        super(unbundlepart, self).__init__(fp)
        self._seekable = util.safehasattr(fp, "seek") and util.safehasattr(fp, "tell")
        self.ui = ui
        # unbundle state attr
        self._headerdata = header
        self._headeroffset = 0
        self._initialized = False
        self.consumed = False
        # part data
        self.id = None
        self.type = None
        self.mandatoryparams: "Tuple[Tuple[str, str], ...]" = ()
        self.advisoryparams: "Tuple[Tuple[str, str], ...]" = ()
        self.params = {}
        self.mandatorykeys = frozenset()
        self._readheader()
        self._mandatory = None
        self._pos = 0

    def _fromheader(self, size: int) -> bytes:
        """return the next <size> byte from the header"""
        offset = self._headeroffset
        data = self._headerdata[offset : (offset + size)]
        self._headeroffset = offset + size
        return data

    def _unpackheader(self, format: str) -> "Any":
        """read given format from header

        This automatically compute the size of the format to read."""
        data = self._fromheader(struct.calcsize(format))
        return _unpack(format, data)

    def _initparams(
        self,
        mandatoryparams: "Iterable[Tuple[str, str]]",
        advisoryparams: "Iterable[Tuple[str, str]]",
    ) -> None:
        """internal function to setup all logic related parameters"""
        # make it read only to prevent people touching it by mistake.
        self.mandatoryparams = tuple(mandatoryparams)
        self.advisoryparams = tuple(advisoryparams)
        # user friendly UI
        self.params = util.sortdict(self.mandatoryparams)
        self.params.update(self.advisoryparams)
        self.mandatorykeys = frozenset(p[0] for p in mandatoryparams)

    def _readheader(self) -> None:
        """read the header and setup the object"""
        typesize = self._unpackheader(_fparttypesize)[0]
        self.type = pycompat.decodeutf8(self._fromheader(typesize))
        indebug(self.ui, 'part type: "%s"' % self.type)
        self.id = self._unpackheader(_fpartid)[0]
        indebug(self.ui, 'part id: "%s"' % pycompat.bytestr(self.id))
        # extract mandatory bit from type
        self.mandatory = self.type != self.type.lower()
        self.type = self.type.lower()
        ## reading parameters
        # param count
        mancount, advcount = self._unpackheader(_fpartparamcount)
        indebug(self.ui, "part parameters: %i" % (mancount + advcount))
        # param size
        fparamsizes = _makefpartparamsizes(mancount + advcount)
        paramsizes = self._unpackheader(fparamsizes)
        # make it a list of couple again
        paramsizes = list(zip(paramsizes[::2], paramsizes[1::2]))
        # split mandatory from advisory
        mansizes = paramsizes[:mancount]
        advsizes = paramsizes[mancount:]
        # retrieve param value
        manparams = []
        for key, value in mansizes:
            key = pycompat.decodeutf8(self._fromheader(key))
            value = pycompat.decodeutf8(self._fromheader(value))
            manparams.append((key, value))
        advparams = []
        for key, value in advsizes:
            key = pycompat.decodeutf8(self._fromheader(key))
            value = pycompat.decodeutf8(self._fromheader(value))
            advparams.append((key, value))
        self._initparams(manparams, advparams)
        ## part payload
        self._payloadstream = util.chunkbuffer(self._payloadchunks())
        # we read the data, tell it
        self._initialized = True

    def _payloadchunks(self) -> "Iterable[bytes]":
        """Generator of decoded chunks in the payload."""
        return decodepayloadchunks(self.ui, self._fp)

    def consume(self) -> None:
        """Read the part payload until completion.

        By consuming the part data, the underlying stream read offset will
        be advanced to the next part (or end of stream).
        """
        if self.consumed:
            return

        chunk = self.read(32768)
        while chunk:
            self._pos += len(chunk)
            chunk = self.read(32768)

    def read(self, size=None):
        # type (Optional[int]) -> bytes
        """read payload data"""
        if not self._initialized:
            self._readheader()
        if size is None:
            data = self._payloadstream.read()
        else:
            data = self._payloadstream.read(size)
        self._pos += len(data)
        if size is None or len(data) < size:
            if not self.consumed and self._pos:
                self.ui.debug("bundle2-input-part: total payload size %i\n" % self._pos)
            self.consumed = True
        return data


class seekableunbundlepart(unbundlepart):
    """A bundle2 part in a bundle that is seekable.

    Regular ``unbundlepart`` instances can only be read once. This class
    extends ``unbundlepart`` to enable bi-directional seeking within the
    part.

    Bundle2 part data consists of framed chunks. Offsets when seeking
    refer to the decoded data, not the offsets in the underlying bundle2
    stream.

    To facilitate quickly seeking within the decoded data, instances of this
    class maintain a mapping between offsets in the underlying stream and
    the decoded payload. This mapping will consume memory in proportion
    to the number of chunks within the payload (which almost certainly
    increases in proportion with the size of the part).
    """

    def __init__(self, ui, header, fp):
        # (payload, file) offsets for chunk starts.
        self._chunkindex = []

        super(seekableunbundlepart, self).__init__(ui, header, fp)

    def _payloadchunks(self, chunknum: int = 0) -> "Iterable[bytes]":
        """seek to specified chunk and start yielding data"""
        if len(self._chunkindex) == 0:
            assert chunknum == 0, "Must start with chunk 0"
            self._chunkindex.append((0, self._tellfp()))
        else:
            assert chunknum < len(self._chunkindex), "Unknown chunk %d" % chunknum
            self._seekfp(self._chunkindex[chunknum][1])

        pos = self._chunkindex[chunknum][0]

        for chunk in decodepayloadchunks(self.ui, self._fp):
            chunknum += 1
            pos += len(chunk)
            if chunknum == len(self._chunkindex):
                self._chunkindex.append((pos, self._tellfp()))

            yield chunk

    def _findchunk(self, pos: int) -> "Tuple[int, int]":
        """for a given payload position, return a chunk number and offset"""
        for chunk, (ppos, fpos) in enumerate(self._chunkindex):
            if ppos == pos:
                return chunk, 0
            elif ppos > pos:
                return chunk - 1, pos - self._chunkindex[chunk - 1][0]
        raise ValueError("Unknown chunk")

    def tell(self) -> int:
        return self._pos

    def seek(self, offset: int, whence: int = os.SEEK_SET) -> None:
        if whence == os.SEEK_SET:
            newpos = offset
        elif whence == os.SEEK_CUR:
            newpos = self._pos + offset
        elif whence == os.SEEK_END:
            if not self.consumed:
                # Can't use self.consume() here because it advances self._pos.
                chunk = self.read(32768)
                while chunk:
                    chunk = self.read(32768)
            newpos = self._chunkindex[-1][0] - offset
        else:
            raise ValueError("Unknown whence value: %r" % (whence,))

        if newpos > self._chunkindex[-1][0] and not self.consumed:
            # Can't use self.consume() here because it advances self._pos.
            chunk = self.read(32768)
            while chunk:
                chunk = self.read(32668)

        if not 0 <= newpos <= self._chunkindex[-1][0]:
            raise ValueError("Offset out of range")

        if self._pos != newpos:
            chunk, internaloffset = self._findchunk(newpos)
            self._payloadstream = util.chunkbuffer(self._payloadchunks(chunk))
            adjust = self.read(internaloffset)
            if len(adjust) != internaloffset:
                raise error.Abort(_("Seek failed\n"))
            self._pos = newpos

    def _seekfp(self, offset: int, whence: int = 0) -> None:
        """move the underlying file pointer

        This method is meant for internal usage by the bundle2 protocol only.
        They directly manipulate the low level stream including bundle2 level
        instruction.

        Do not use it to implement higher-level logic or methods."""
        if self._seekable:
            return self._fp.seek(offset, whence)
        else:
            raise NotImplementedError(_("File pointer is not seekable"))

    def _tellfp(self) -> "Optional[int]":
        """return the file offset, or None if file is not seekable

        This method is meant for internal usage by the bundle2 protocol only.
        They directly manipulate the low level stream including bundle2 level
        instruction.

        Do not use it to implement higher-level logic or methods."""
        if self._seekable:
            try:
                return self._fp.tell()
            except IOError as e:
                if e.errno == errno.ESPIPE:
                    self._seekable = False
                else:
                    raise
        return None


# These are only the static capabilities.
# Check the 'getrepocaps' function for the rest.
capabilities = {
    "HG20": (),
    "bookmarks": (),
    "error": ("abort", "unsupportedcontent", "pushraced", "pushkey"),
    "listkeys": (),
    "pushkey": (),
    "digests": tuple(sorted(util.DIGESTS.keys())),
    "remote-changegroup": ("http", "https"),
    "phases": ("heads",),
}


def getrepocaps(
    repo: "Any", allowpushback: bool = False
) -> "Dict[str, Tuple[str, ...]]":
    """return the bundle2 capabilities for a given repo

    Exists to allow extensions (like evolution) to mutate the capabilities.
    """
    caps = capabilities.copy()
    caps["changegroup"] = tuple(sorted(changegroup.supportedincomingversions(repo)))
    if allowpushback:
        caps["pushback"] = ()
    if "phases" in repo.ui.configlist("devel", "legacy.exchange"):
        caps.pop("phases")
    return caps


def bundle2caps(remote: "Any") -> "Dict[str, Tuple[str, ...]]":
    """return the bundle capabilities of a peer as dict"""
    raw = remote.capable("bundle2")
    if not raw and raw != "":
        return {}
    capsblob = urllibcompat.unquote(remote.capable("bundle2"))
    return decodecaps(capsblob)


def obsmarkersversion(caps):
    # type (Dict[str, Tuple[str, ...]]) -> Iterable[int]
    """extract the list of supported obsmarkers versions from a bundle2caps dict"""
    obscaps = caps.get("obsmarkers", ())
    return [int(c[1:]) for c in obscaps if c.startswith("V")]


def writenewbundle(
    ui: "Any",
    repo: "Any",
    source: str,
    filename: str,
    bundletype: str,
    outgoing: "discovery.outgoing",
    opts: "Dict[str, Any]",
    vfs: "Optional[abstractvfs]" = None,
    compression: "Optional[str]" = None,
    compopts: "Optional[Dict]" = None,
) -> str:
    if bundletype.startswith("HG10"):
        cg = changegroup.makechangegroup(repo, outgoing, "01", source)
        return writebundle(
            ui,
            cg,
            filename,
            bundletype,
            vfs=vfs,
            compression=compression,
            compopts=compopts,
        )
    elif not bundletype.startswith("HG20"):
        raise error.ProgrammingError("unknown bundle type: %s" % bundletype)

    caps = getrepocaps(repo)
    bundle = bundle20(ui, caps)
    bundle.setcompression(compression, compopts)
    _addpartsfromopts(ui, repo, bundle, source, outgoing, opts, caps)
    chunkiter = bundle.getchunks()

    return changegroup.writechunks(ui, chunkiter, filename, vfs=vfs)


def _addpartsfromopts(
    ui: "Any",
    repo: "Any",
    bundler: "bundle20",
    source: str,
    outgoing: "discovery.outgoing",
    opts: "Dict[str, Any]",
    caps: "Dict[str, Tuple[str, ...]]",
) -> None:
    # We should eventually reconcile this logic with the one behind
    # 'exchange.getbundle2partsgenerator'.
    #
    # The type of input from 'getbundle' and 'writenewbundle' are a bit
    # different right now. So we keep them separated for now for the sake of
    # simplicity.

    # we always want a changegroup in such bundle
    cgversion = opts.get("cg.version")
    if cgversion is None:
        cgversion = changegroup.safeversion(repo)
    cg = changegroup.makechangegroup(repo, outgoing, cgversion, source, b2caps=caps)
    part = bundler.newpart("changegroup", data=cg.getchunks())
    part.addparam("version", cg.version)
    if "clcount" in cg.extras:
        part.addparam("nbchanges", "%d" % cg.extras["clcount"], mandatory=False)
    if opts.get("phases") and repo.revs("%ln and secret()", outgoing.missingheads):
        part.addparam("targetphase", "%d" % phases.secret, mandatory=False)

    if opts.get("phases", False):
        headsbyphase = phases.subsetphaseheads(repo, outgoing.missing)
        phasedata = phases.binaryencode(headsbyphase)
        bundler.newpart("phase-heads", data=phasedata)


def buildobsmarkerspart(bundler, markers):
    """add an obsmarker part to the bundler with <markers>

    No part is created if markers is empty.
    Raises ValueError if the bundler doesn't support any known obsmarker format.
    """
    if not markers:
        return None

    remoteversions = obsmarkersversion(bundler.capabilities)
    version = obsolete.commonversion(remoteversions)
    if version is None:
        raise ValueError("bundler does not support common obsmarker format")
    stream = obsolete.encodemarkers(markers, True, version=version)
    return bundler.newpart("obsmarkers", data=stream)


def writebundle(
    ui: "Any",
    cg: "changegroup.cg1unpacker",
    filename: str,
    bundletype: str,
    vfs: "Optional[abstractvfs]" = None,
    compression: "Optional[str]" = None,
    compopts: "Optional[Dict]" = None,
) -> str:
    """Write a bundle file and return its filename.

    Existing files will not be overwritten.
    If no filename is specified, a temporary file is created.
    bz2 compression can be turned off.
    The bundle file will be deleted in case of errors.
    """

    if bundletype == "HG20":
        bundle = bundle20(ui)
        bundle.setcompression(compression, compopts)
        part = bundle.newpart("changegroup", data=cg.getchunks())
        part.addparam("version", cg.version)
        if "clcount" in cg.extras:
            part.addparam("nbchanges", "%d" % cg.extras["clcount"], mandatory=False)
        chunkiter = bundle.getchunks()
    else:
        # compression argument is only for the bundle2 case
        assert compression is None
        if cg.version != "01":
            raise error.Abort(_("old bundle types only supports v1 " "changegroups"))
        header, comp = bundletypes[bundletype]
        if comp not in util.compengines.supportedbundletypes:
            raise error.Abort(_("unknown stream compression type: %s") % comp)
        compengine = util.compengines.forbundletype(comp)

        def chunkiter():
            yield header
            for chunk in compengine.compressstream(cg.getchunks(), compopts):
                yield chunk

        chunkiter = chunkiter()

    # parse the changegroup data, otherwise we will block
    # in case of sshrepo because we don't know the end of the stream
    return changegroup.writechunks(ui, chunkiter, filename, vfs=vfs)


def combinechangegroupresults(op: "bundleoperation") -> int:
    """logic to combine 0 or more addchangegroup results into one"""
    results = [r.get("return", 0) for r in op.records["changegroup"]]
    changedheads = 0
    result = 1
    for ret in results:
        # If any changegroup result is 0, return 0
        if ret == 0:
            result = 0
            break
        if ret < -1:
            changedheads += ret + 1
        elif ret > 1:
            changedheads += ret - 1
    if changedheads > 0:
        result = 1 + changedheads
    elif changedheads < 0:
        result = -1 + changedheads
    return result


@parthandler("changegroup", ("version", "nbchanges", "treemanifest", "targetphase"))
def handlechangegroup(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """apply a changegroup part on the repo

    This is a very early implementation that will massive rework before being
    inflicted to any end-user.
    """
    tr = op.gettransaction()
    unpackerversion = inpart.params.get("version", "01")
    # We should raise an appropriate exception here
    cg = changegroup.getunbundler(unpackerversion, inpart, None)
    # the source and url passed here are overwritten by the one contained in
    # the transaction.hookargs argument. So 'bundle2' is a placeholder
    nbchangesets = None
    if "nbchanges" in inpart.params:
        nbchangesets = int(inpart.params.get("nbchanges"))
    if "treemanifest" in inpart.params and "treemanifest" not in op.repo.requirements:
        if len(op.repo.changelog) != 0:
            raise error.Abort(
                _(
                    "bundle contains tree manifests, but local repo is "
                    "non-empty and does not use tree manifests"
                )
            )
        op.repo.requirements.add("treemanifest")
        op.repo._applyopenerreqs()
        op.repo._writerequirements()
    extrakwargs = {}
    targetphase = inpart.params.get("targetphase")
    if targetphase is not None:
        extrakwargs["targetphase"] = int(targetphase)
    ret = _processchangegroup(
        op, cg, tr, "bundle2", "bundle2", expectedtotal=nbchangesets, **extrakwargs
    )
    reply = op.reply
    if reply is not None:
        # This is definitely not the final form of this
        # return. But one need to start somewhere.
        part = reply.newpart("reply:changegroup", mandatory=False)
        part.addparam("in-reply-to", pycompat.bytestr(inpart.id), mandatory=False)
        part.addparam("return", "%i" % ret, mandatory=False)
    assert not inpart.read()


_remotechangegroupparams = tuple(
    ["url", "size", "digests"] + ["digest:%s" % k for k in util.DIGESTS.keys()]
)


@parthandler("remote-changegroup", _remotechangegroupparams)
def handleremotechangegroup(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """apply a bundle10 on the repo, given an url and validation information

    All the information about the remote bundle to import are given as
    parameters. The parameters include:
      - url: the url to the bundle10.
      - size: the bundle10 file size. It is used to validate what was
        retrieved by the client matches the server knowledge about the bundle.
      - digests: a space separated list of the digest types provided as
        parameters.
      - digest:<digest-type>: the hexadecimal representation of the digest with
        that name. Like the size, it is used to validate what was retrieved by
        the client matches what the server knows about the bundle.

    When multiple digest types are given, all of them are checked.
    """
    try:
        raw_url = inpart.params["url"]
    except KeyError:
        raise error.Abort(_('remote-changegroup: missing "%s" param') % "url")
    parsed_url = util.url(raw_url)
    if parsed_url.scheme not in capabilities["remote-changegroup"]:
        raise error.Abort(
            _("remote-changegroup does not support %s urls") % parsed_url.scheme
        )

    try:
        size = int(inpart.params["size"])
    except ValueError:
        raise error.Abort(
            _('remote-changegroup: invalid value for param "%s"') % "size"
        )
    except KeyError:
        raise error.Abort(_('remote-changegroup: missing "%s" param') % "size")

    digests = {}
    for typ in inpart.params.get("digests", "").split():
        param = "digest:%s" % typ
        try:
            value = inpart.params[param]
        except KeyError:
            raise error.Abort(_('remote-changegroup: missing "%s" param') % param)
        digests[typ] = value

    real_part = util.digestchecker(url.open(op.ui, raw_url), size, digests)

    tr = op.gettransaction()
    from . import exchange

    cg = exchange.readbundle(op.repo.ui, real_part, raw_url)
    if not isinstance(cg, changegroup.cg1unpacker):
        raise error.Abort(
            _("%s: not a bundle version 1.0") % util.hidepassword(raw_url)
        )
    ret = _processchangegroup(op, cg, tr, "bundle2", "bundle2")
    reply = op.reply
    if reply is not None:
        # This is definitely not the final form of this
        # return. But one need to start somewhere.
        part = reply.newpart("reply:changegroup")
        part.addparam("in-reply-to", pycompat.bytestr(inpart.id), mandatory=False)
        part.addparam("return", "%i" % ret, mandatory=False)
    try:
        real_part.validate()
    except error.Abort as e:
        raise error.Abort(
            _("bundle at %s is corrupted:\n%s") % (util.hidepassword(raw_url), str(e))
        )
    assert not inpart.read()


@parthandler("reply:changegroup", ("return", "in-reply-to"))
def handlereplychangegroup(op: "bundleoperation", inpart: "unbundlepart") -> None:
    ret = int(inpart.params["return"])
    replyto = int(inpart.params["in-reply-to"])
    op.records.add("changegroup", {"return": ret}, replyto)


@parthandler("check:bookmarks")
def handlecheckbookmarks(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """check location of bookmarks

    This part is to be used to detect push race regarding bookmark, it
    contains binary encoded (bookmark, node) tuple. If the local state does
    not marks the one in the part, a PushRaced exception is raised
    """
    bookdata = bookmarks.binarydecode(inpart)

    msgstandard = (
        "repository changed while pushing - please try again "
        '(bookmark "%s" move from %s to %s)'
    )
    msgmissing = (
        "repository changed while pushing - please try again "
        '(bookmark "%s" is missing, expected %s)'
    )
    msgexist = (
        "repository changed while pushing - please try again "
        '(bookmark "%s" set on %s, expected missing)'
    )
    for book, node in bookdata:
        currentnode = op.repo._bookmarks.get(book)
        if currentnode != node:
            if node is None:
                finalmsg = msgexist % (book, nodemod.short(currentnode))
            elif currentnode is None:
                finalmsg = msgmissing % (book, nodemod.short(node))
            else:
                finalmsg = msgstandard % (
                    book,
                    nodemod.short(node),
                    nodemod.short(currentnode),
                )
            raise error.PushRaced(finalmsg)


@parthandler("check:phases")
def handlecheckphases(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """check that phase boundaries of the repository did not change

    This is used to detect a push race.
    """
    phasetonodes = phases.binarydecode(inpart)
    unfi = op.repo
    cl = unfi.changelog
    phasecache = unfi._phasecache
    msg = (
        "repository changed while pushing - please try again " "(%s is %s expected %s)"
    )
    for expectedphase, nodes in enumerate(phasetonodes):
        for n in nodes:
            actualphase = phasecache.phase(unfi, cl.rev(n))
            if actualphase != expectedphase:
                finalmsg = msg % (
                    nodemod.short(n),
                    phases.phasenames[actualphase],
                    phases.phasenames[expectedphase],
                )
                raise error.PushRaced(finalmsg)


@parthandler("output")
def handleoutput(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """forward output captured on the server to the client"""
    for line in inpart.read().splitlines():
        op.ui.status(_("remote: %s\n") % pycompat.decodeutf8(line))


@parthandler("replycaps")
def handlereplycaps(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """Notify that a reply bundle should be created

    The payload contains the capabilities information for the reply"""
    caps = decodecaps(pycompat.decodeutf8(inpart.read()))
    if op.reply is None:
        op.reply = bundle20(op.ui, caps)


class AbortFromPart(error.Abort):
    """Sub-class of Abort that denotes an error from a bundle2 part."""


def createerrorpart(msg, hint=None, mandatory=True):
    # type (str, Optional[str], bool) -> bundlepart
    """Creates an error abort bundle part. In particular, it enforces the
    message length maximum."""
    maxarglen = 255
    manargs = []
    advargs = []

    if len(msg) > maxarglen:
        msg = msg[: maxarglen - 3] + "..."
    manargs.append(("message", msg))

    if hint is not None:
        if len(hint) > maxarglen:
            hint = hint[: maxarglen - 3] + "..."
        advargs.append(("hint", hint))

    return bundlepart("error:abort", manargs, advargs, mandatory=mandatory)


@parthandler("error:abort", ("message", "hint"))
def handleerrorabort(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """Used to transmit abort error over the wire"""
    raise AbortFromPart(inpart.params["message"], hint=inpart.params.get("hint"))


@parthandler("error:pushkey", ("namespace", "key", "new", "old", "ret", "in-reply-to"))
def handleerrorpushkey(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """Used to transmit failure of a mandatory pushkey over the wire"""
    kwargs = {}
    for name in ("namespace", "key", "new", "old", "ret"):
        value = inpart.params.get(name)
        if value is not None:
            kwargs[name] = value
    raise error.PushkeyFailed(inpart.params["in-reply-to"], **kwargs)


@parthandler("error:unsupportedcontent", ("parttype", "params"))
def handleerrorunsupportedcontent(
    op: "bundleoperation", inpart: "unbundlepart"
) -> None:
    """Used to transmit unknown content error over the wire"""
    kwargs = {}
    parttype = inpart.params.get("parttype")
    if parttype is not None:
        kwargs["parttype"] = parttype
    params = inpart.params.get("params")
    if params is not None:
        kwargs["params"] = params.split("\0")

    raise error.BundleUnknownFeatureError(**kwargs)


@parthandler("error:pushraced", ("message",))
def handleerrorpushraced(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """Used to transmit push race error over the wire"""
    raise error.ResponseError(_("push failed:"), inpart.params["message"])


@parthandler("listkeys", ("namespace",))
def handlelistkeys(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """retrieve pushkey namespace content stored in a bundle2"""
    namespace = inpart.params["namespace"]
    r = pushkey.decodekeys(inpart.read())
    op.records.add("listkeys", (namespace, r))


@parthandler("pushkey", ("namespace", "key", "old", "new"))
def handlepushkey(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """process a pushkey request"""
    namespace = inpart.params["namespace"]
    key = inpart.params["key"]
    old = inpart.params["old"]
    new = inpart.params["new"]

    # The lock may be lazy, so grab it to ensure that we have the lock before
    # performing the pushkey.
    op.gettransaction()
    ret = op.repo.pushkey(namespace, key, old, new)
    record = {"namespace": namespace, "key": key, "old": old, "new": new}
    op.records.add("pushkey", record)
    reply = op.reply
    if reply is not None:
        rpart = reply.newpart("reply:pushkey")
        rpart.addparam("in-reply-to", pycompat.bytestr(inpart.id), mandatory=False)
        rpart.addparam("return", "%i" % ret, mandatory=False)
    if inpart.mandatory and not ret:
        kwargs = {}
        for key in ("namespace", "key", "new", "old", "ret"):
            if key in inpart.params:
                kwargs[key] = inpart.params[key]
        raise error.PushkeyFailed(partid=str(inpart.id), **kwargs)


@parthandler("bookmarks")
def handlebookmark(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """transmit bookmark information

    The part contains binary encoded bookmark information.

    The exact behavior of this part can be controlled by the 'bookmarks' mode
    on the bundle operation.

    When mode is 'apply' (the default) the bookmark information is applied as
    is to the unbundling repository. Make sure a 'check:bookmarks' part is
    issued earlier to check for push races in such update. This behavior is
    suitable for pushing.

    When mode is 'records', the information is recorded into the 'bookmarks'
    records of the bundle operation. This behavior is suitable for pulling.
    """
    changes = bookmarks.binarydecode(inpart)

    pushkeycompat = op.repo.ui.configbool("server", "bookmarks-pushkey-compat")
    bookmarksmode = op.modes.get("bookmarks", "apply")

    if bookmarksmode == "apply":
        tr = op.gettransaction()
        bookstore = op.repo._bookmarks
        if pushkeycompat:
            allhooks = []
            for book, node in changes:
                hookargs = tr.hookargs.copy()
                hookargs["pushkeycompat"] = "1"
                hookargs["namespace"] = "bookmark"
                hookargs["key"] = book
                hookargs["old"] = nodemod.hex(bookstore.get(book, b""))
                hookargs["new"] = nodemod.hex(node if node is not None else b"")
                allhooks.append(hookargs)

            for hookargs in allhooks:
                op.repo.hook("prepushkey", throw=True, **hookargs)

        bookstore.applychanges(op.repo, op.gettransaction(), changes)

        if pushkeycompat:

            def runhook():
                for hookargs in allhooks:
                    op.repo.hook("prepushkey", **hookargs)

            op.repo._afterlock(runhook)

    elif bookmarksmode == "records":
        for book, node in changes:
            record = {"bookmark": book, "node": node}
            op.records.add("bookmarks", record)
    else:
        raise error.ProgrammingError("unknown bookmark mode: %s" % bookmarksmode)


@parthandler("phase-heads")
def handlephases(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """apply phases from bundle part to repo"""
    headsbyphase = phases.binarydecode(inpart)
    phases.updatephases(op.repo, op.gettransaction, headsbyphase)


@parthandler("reply:pushkey", ("return", "in-reply-to"))
def handlepushkeyreply(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """retrieve the result of a pushkey request"""
    ret = int(inpart.params["return"])
    partid = int(inpart.params["in-reply-to"])
    op.records.add("pushkey", {"return": ret}, partid)


@parthandler("obsmarkers")
def handleobsmarker(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """add a stream of obsmarkers to the repo"""
    markerdata = inpart.read()
    _importmarkers(markerdata)
    op.repo.invalidatevolatilesets()


# wrapped by pushrebase
def _importmarkers(markerdata):
    pass


@parthandler("reply:obsmarkers", ("new", "in-reply-to"))
def handleobsmarkerreply(op: "bundleoperation", inpart: "unbundlepart") -> None:
    """retrieve the result of a pushkey request"""
    ret = int(inpart.params["new"])
    partid = int(inpart.params["in-reply-to"])
    op.records.add("obsmarkers", {"new": ret}, partid)


@parthandler("pushvars")
def bundle2getvars(op: "bundleoperation", part: "unbundlepart") -> None:
    """unbundle a bundle2 containing shellvars on the server"""
    # An option to disable unbundling on server-side for security reasons
    if op.ui.configbool("push", "pushvars.server"):
        hookargs = {}
        for key, value in part.advisoryparams:
            key = key.upper()
            # We want pushed variables to have USERVAR_ prepended so we know
            # they came from the --pushvar flag.
            key = "USERVAR_" + key
            hookargs[key] = value
        op.addhookargs(hookargs)
