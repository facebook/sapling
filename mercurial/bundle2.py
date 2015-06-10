# bundle2.py - generic container format to transmit arbitrary data.
#
# Copyright 2013 Facebook, Inc.
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

import errno
import sys
import util
import struct
import urllib
import string
import obsolete
import pushkey
import url
import re

import changegroup, error, tags
from i18n import _

_pack = struct.pack
_unpack = struct.unpack

_fstreamparamsize = '>i'
_fpartheadersize = '>i'
_fparttypesize = '>B'
_fpartid = '>I'
_fpayloadsize = '>i'
_fpartparamcount = '>BB'

preferedchunksize = 4096

_parttypeforbidden = re.compile('[^a-zA-Z0-9_:-]')

def outdebug(ui, message):
    """debug regarding output stream (bundling)"""
    if ui.configbool('devel', 'bundle2.debug', False):
        ui.debug('bundle2-output: %s\n' % message)

def indebug(ui, message):
    """debug on input stream (unbundling)"""
    if ui.configbool('devel', 'bundle2.debug', False):
        ui.debug('bundle2-input: %s\n' % message)

def validateparttype(parttype):
    """raise ValueError if a parttype contains invalid character"""
    if _parttypeforbidden.search(parttype):
        raise ValueError(parttype)

def _makefpartparamsizes(nbparams):
    """return a struct format to read part parameter sizes

    The number parameters is variable so we need to build that format
    dynamically.
    """
    return '>'+('BB'*nbparams)

parthandlermapping = {}

def parthandler(parttype, params=()):
    """decorator that register a function as a bundle2 part handler

    eg::

        @parthandler('myparttype', ('mandatory', 'param', 'handled'))
        def myparttypehandler(...):
            '''process a part of type "my part".'''
            ...
    """
    validateparttype(parttype)
    def _decorator(func):
        lparttype = parttype.lower() # enforce lower case matching.
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
    """

    def __init__(self, repo, transactiongetter, captureoutput=True):
        self.repo = repo
        self.ui = repo.ui
        self.records = unbundlerecords()
        self.gettransaction = transactiongetter
        self.reply = None
        self.captureoutput = captureoutput

class TransactionUnavailable(RuntimeError):
    pass

def _notransaction():
    """default method to get a transaction while processing a bundle

    Raise an exception to highlight the fact that no transaction was expected
    to be created"""
    raise TransactionUnavailable()

def processbundle(repo, unbundler, transactiongetter=None, op=None):
    """This function process a bundle, apply effect to/from a repo

    It iterates over each part then searches for and uses the proper handling
    code to process the part. Parts are processed in order.

    This is very early version of this function that will be strongly reworked
    before final usage.

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
        msg = ['bundle2-input-bundle:']
        if unbundler.params:
            msg.append(' %i params')
        if op.gettransaction is None:
            msg.append(' no-transaction')
        else:
            msg.append(' with-transaction')
        msg.append('\n')
        repo.ui.debug(''.join(msg))
    iterparts = enumerate(unbundler.iterparts())
    part = None
    nbpart = 0
    try:
        for nbpart, part in iterparts:
            _processpart(op, part)
    except BaseException, exc:
        for nbpart, part in iterparts:
            # consume the bundle content
            part.seek(0, 2)
        # Small hack to let caller code distinguish exceptions from bundle2
        # processing from processing the old format. This is mostly
        # needed to handle different return codes to unbundle according to the
        # type of bundle. We should probably clean up or drop this return code
        # craziness in a future version.
        exc.duringunbundle2 = True
        salvaged = []
        replycaps = None
        if op.reply is not None:
            salvaged = op.reply.salvageoutput()
            replycaps = op.reply.capabilities
        exc._replycaps = replycaps
        exc._bundle2salvagedoutput = salvaged
        raise
    finally:
        repo.ui.debug('bundle2-input-bundle: %i parts total\n' % nbpart)

    return op

def _processpart(op, part):
    """process a single part from a bundle

    The part is guaranteed to have been fully consumed when the function exits
    (even if an exception is raised)."""
    status = 'unknown' # used by debug output
    try:
        try:
            handler = parthandlermapping.get(part.type)
            if handler is None:
                status = 'unsupported-type'
                raise error.UnsupportedPartError(parttype=part.type)
            indebug(op.ui, 'found a handler for part %r' % part.type)
            unknownparams = part.mandatorykeys - handler.params
            if unknownparams:
                unknownparams = list(unknownparams)
                unknownparams.sort()
                status = 'unsupported-params (%s)' % unknownparams
                raise error.UnsupportedPartError(parttype=part.type,
                                               params=unknownparams)
            status = 'supported'
        except error.UnsupportedPartError, exc:
            if part.mandatory: # mandatory parts
                raise
            indebug(op.ui, 'ignoring unsupported advisory part %s' % exc)
            return # skip to part processing
        finally:
            if op.ui.debugflag:
                msg = ['bundle2-input-part: "%s"' % part.type]
                if not part.mandatory:
                    msg.append(' (advisory)')
                nbmp = len(part.mandatorykeys)
                nbap = len(part.params) - nbmp
                if nbmp or nbap:
                    msg.append(' (params:')
                    if nbmp:
                        msg.append(' %i mandatory' % nbmp)
                    if nbap:
                        msg.append(' %i advisory' % nbmp)
                    msg.append(')')
                msg.append(' %s\n' % status)
                op.ui.debug(''.join(msg))

        # handler is called outside the above try block so that we don't
        # risk catching KeyErrors from anything other than the
        # parthandlermapping lookup (any KeyError raised by handler()
        # itself represents a defect of a different variety).
        output = None
        if op.captureoutput and op.reply is not None:
            op.ui.pushbuffer(error=True, subproc=True)
            output = ''
        try:
            handler(op, part)
        finally:
            if output is not None:
                output = op.ui.popbuffer()
            if output:
                outpart = op.reply.newpart('output', data=output,
                                           mandatory=False)
                outpart.addparam('in-reply-to', str(part.id), mandatory=False)
    finally:
        # consume the part content to not corrupt the stream.
        part.seek(0, 2)


def decodecaps(blob):
    """decode a bundle2 caps bytes blob into a dictionary

    The blob is a list of capabilities (one per line)
    Capabilities may have values using a line of the form::

        capability=value1,value2,value3

    The values are always a list."""
    caps = {}
    for line in blob.splitlines():
        if not line:
            continue
        if '=' not in line:
            key, vals = line, ()
        else:
            key, vals = line.split('=', 1)
            vals = vals.split(',')
        key = urllib.unquote(key)
        vals = [urllib.unquote(v) for v in vals]
        caps[key] = vals
    return caps

def encodecaps(caps):
    """encode a bundle2 caps dictionary into a bytes blob"""
    chunks = []
    for ca in sorted(caps):
        vals = caps[ca]
        ca = urllib.quote(ca)
        vals = [urllib.quote(v) for v in vals]
        if vals:
            ca = "%s=%s" % (ca, ','.join(vals))
        chunks.append(ca)
    return '\n'.join(chunks)

class bundle20(object):
    """represent an outgoing bundle2 container

    Use the `addparam` method to add stream level parameter. and `newpart` to
    populate it. Then call `getchunks` to retrieve all the binary chunks of
    data that compose the bundle2 container."""

    _magicstring = 'HG20'

    def __init__(self, ui, capabilities=()):
        self.ui = ui
        self._params = []
        self._parts = []
        self.capabilities = dict(capabilities)

    @property
    def nbparts(self):
        """total number of parts added to the bundler"""
        return len(self._parts)

    # methods used to defines the bundle2 content
    def addparam(self, name, value=None):
        """add a stream level parameter"""
        if not name:
            raise ValueError('empty parameter name')
        if name[0] not in string.letters:
            raise ValueError('non letter first character: %r' % name)
        self._params.append((name, value))

    def addpart(self, part):
        """add a new part to the bundle2 container

        Parts contains the actual applicative payload."""
        assert part.id is None
        part.id = len(self._parts) # very cheap counter
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
    def getchunks(self):
        if self.ui.debugflag:
            msg = ['bundle2-output-bundle: "%s",' % self._magicstring]
            if self._params:
                msg.append(' (%i params)' % len(self._params))
            msg.append(' %i parts total\n' % len(self._parts))
            self.ui.debug(''.join(msg))
        outdebug(self.ui, 'start emission of %s stream' % self._magicstring)
        yield self._magicstring
        param = self._paramchunk()
        outdebug(self.ui, 'bundle parameter: %s' % param)
        yield _pack(_fstreamparamsize, len(param))
        if param:
            yield param

        outdebug(self.ui, 'start of parts')
        for part in self._parts:
            outdebug(self.ui, 'bundle part: "%s"' % part.type)
            for chunk in part.getchunks(ui=self.ui):
                yield chunk
        outdebug(self.ui, 'end of bundle')
        yield _pack(_fpartheadersize, 0)

    def _paramchunk(self):
        """return a encoded version of all stream parameters"""
        blocks = []
        for par, value in self._params:
            par = urllib.quote(par)
            if value is not None:
                value = urllib.quote(value)
                par = '%s=%s' % (par, value)
            blocks.append(par)
        return ' '.join(blocks)

    def salvageoutput(self):
        """return a list with a copy of all output parts in the bundle

        This is meant to be used during error handling to make sure we preserve
        server output"""
        salvaged = []
        for part in self._parts:
            if part.type.startswith('output'):
                salvaged.append(part.copy())
        return salvaged


class unpackermixin(object):
    """A mixin to extract bytes and struct data from a stream"""

    def __init__(self, fp):
        self._fp = fp
        self._seekable = (util.safehasattr(fp, 'seek') and
                          util.safehasattr(fp, 'tell'))

    def _unpack(self, format):
        """unpack this struct format from the stream"""
        data = self._readexact(struct.calcsize(format))
        return _unpack(format, data)

    def _readexact(self, size):
        """read exactly <size> bytes from the stream"""
        return changegroup.readexactly(self._fp, size)

    def seek(self, offset, whence=0):
        """move the underlying file pointer"""
        if self._seekable:
            return self._fp.seek(offset, whence)
        else:
            raise NotImplementedError(_('File pointer is not seekable'))

    def tell(self):
        """return the file offset, or None if file is not seekable"""
        if self._seekable:
            try:
                return self._fp.tell()
            except IOError, e:
                if e.errno == errno.ESPIPE:
                    self._seekable = False
                else:
                    raise
        return None

    def close(self):
        """close underlying file"""
        if util.safehasattr(self._fp, 'close'):
            return self._fp.close()

def getunbundler(ui, fp, header=None):
    """return a valid unbundler object for a given header"""
    if header is None:
        header = changegroup.readexactly(fp, 4)
    magic, version = header[0:2], header[2:4]
    if magic != 'HG':
        raise util.Abort(_('not a Mercurial bundle'))
    unbundlerclass = formatmap.get(version)
    if unbundlerclass is None:
        raise util.Abort(_('unknown bundle version %s') % version)
    unbundler = unbundlerclass(ui, fp)
    indebug(ui, 'start processing of %s stream' % header)
    return unbundler

class unbundle20(unpackermixin):
    """interpret a bundle2 stream

    This class is fed with a binary stream and yields parts through its
    `iterparts` methods."""

    def __init__(self, ui, fp):
        """If header is specified, we do not read it out of the stream."""
        self.ui = ui
        super(unbundle20, self).__init__(fp)

    @util.propertycache
    def params(self):
        """dictionary of stream level parameters"""
        indebug(self.ui, 'reading bundle2 stream parameters')
        params = {}
        paramssize = self._unpack(_fstreamparamsize)[0]
        if paramssize < 0:
            raise error.BundleValueError('negative bundle param size: %i'
                                         % paramssize)
        if paramssize:
            for p in self._readexact(paramssize).split(' '):
                p = p.split('=', 1)
                p = [urllib.unquote(i) for i in p]
                if len(p) < 2:
                    p.append(None)
                self._processparam(*p)
                params[p[0]] = p[1]
        return params

    def _processparam(self, name, value):
        """process a parameter, applying its effect if needed

        Parameter starting with a lower case letter are advisory and will be
        ignored when unknown.  Those starting with an upper case letter are
        mandatory and will this function will raise a KeyError when unknown.

        Note: no option are currently supported. Any input will be either
              ignored or failing.
        """
        if not name:
            raise ValueError('empty parameter name')
        if name[0] not in string.letters:
            raise ValueError('non letter first character: %r' % name)
        # Some logic will be later added here to try to process the option for
        # a dict of known parameter.
        if name[0].islower():
            indebug(self.ui, "ignoring unknown parameter %r" % name)
        else:
            raise error.UnsupportedPartError(params=(name,))


    def iterparts(self):
        """yield all parts contained in the stream"""
        # make sure param have been loaded
        self.params
        indebug(self.ui, 'start extraction of bundle2 parts')
        headerblock = self._readpartheader()
        while headerblock is not None:
            part = unbundlepart(self.ui, headerblock, self._fp)
            yield part
            part.seek(0, 2)
            headerblock = self._readpartheader()
        indebug(self.ui, 'end of bundle2 stream')

    def _readpartheader(self):
        """reads a part header size and return the bytes blob

        returns None if empty"""
        headersize = self._unpack(_fpartheadersize)[0]
        if headersize < 0:
            raise error.BundleValueError('negative part header size: %i'
                                         % headersize)
        indebug(self.ui, 'part header size: %i' % headersize)
        if headersize:
            return self._readexact(headersize)
        return None

    def compressed(self):
        return False

formatmap = {'20': unbundle20}

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

    def __init__(self, parttype, mandatoryparams=(), advisoryparams=(),
                 data='', mandatory=True):
        validateparttype(parttype)
        self.id = None
        self.type = parttype
        self._data = data
        self._mandatoryparams = list(mandatoryparams)
        self._advisoryparams = list(advisoryparams)
        # checking for duplicated entries
        self._seenparams = set()
        for pname, __ in self._mandatoryparams + self._advisoryparams:
            if pname in self._seenparams:
                raise RuntimeError('duplicated params: %s' % pname)
            self._seenparams.add(pname)
        # status of the part's generation:
        # - None: not started,
        # - False: currently generated,
        # - True: generation done.
        self._generated = None
        self.mandatory = mandatory

    def copy(self):
        """return a copy of the part

        The new part have the very same content but no partid assigned yet.
        Parts with generated data cannot be copied."""
        assert not util.safehasattr(self.data, 'next')
        return self.__class__(self.type, self._mandatoryparams,
                              self._advisoryparams, self._data, self.mandatory)

    # methods used to defines the part content
    def __setdata(self, data):
        if self._generated is not None:
            raise error.ReadOnlyPartError('part is being generated')
        self._data = data
    def __getdata(self):
        return self._data
    data = property(__getdata, __setdata)

    @property
    def mandatoryparams(self):
        # make it an immutable tuple to force people through ``addparam``
        return tuple(self._mandatoryparams)

    @property
    def advisoryparams(self):
        # make it an immutable tuple to force people through ``addparam``
        return tuple(self._advisoryparams)

    def addparam(self, name, value='', mandatory=True):
        if self._generated is not None:
            raise error.ReadOnlyPartError('part is being generated')
        if name in self._seenparams:
            raise ValueError('duplicated params: %s' % name)
        self._seenparams.add(name)
        params = self._advisoryparams
        if mandatory:
            params = self._mandatoryparams
        params.append((name, value))

    # methods used to generates the bundle2 stream
    def getchunks(self, ui):
        if self._generated is not None:
            raise RuntimeError('part can only be consumed once')
        self._generated = False

        if ui.debugflag:
            msg = ['bundle2-output-part: "%s"' % self.type]
            if not self.mandatory:
                msg.append(' (advisory)')
            nbmp = len(self.mandatoryparams)
            nbap = len(self.advisoryparams)
            if nbmp or nbap:
                msg.append(' (params:')
                if nbmp:
                    msg.append(' %i mandatory' % nbmp)
                if nbap:
                    msg.append(' %i advisory' % nbmp)
                msg.append(')')
            if not self.data:
                msg.append(' empty payload')
            elif util.safehasattr(self.data, 'next'):
                msg.append(' streamed payload')
            else:
                msg.append(' %i bytes payload' % len(self.data))
            msg.append('\n')
            ui.debug(''.join(msg))

        #### header
        if self.mandatory:
            parttype = self.type.upper()
        else:
            parttype = self.type.lower()
        outdebug(ui, 'part %s: "%s"' % (self.id, parttype))
        ## parttype
        header = [_pack(_fparttypesize, len(parttype)),
                  parttype, _pack(_fpartid, self.id),
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
        paramsizes = _pack(_makefpartparamsizes(len(parsizes) / 2), *parsizes)
        header.append(paramsizes)
        # key, value
        for key, value in manpar:
            header.append(key)
            header.append(value)
        for key, value in advpar:
            header.append(key)
            header.append(value)
        ## finalize header
        headerchunk = ''.join(header)
        outdebug(ui, 'header chunk size: %i' % len(headerchunk))
        yield _pack(_fpartheadersize, len(headerchunk))
        yield headerchunk
        ## payload
        try:
            for chunk in self._payloadchunks():
                outdebug(ui, 'payload chunk size: %i' % len(chunk))
                yield _pack(_fpayloadsize, len(chunk))
                yield chunk
        except BaseException, exc:
            # backup exception data for later
            ui.debug('bundle2-input-stream-interrupt: encoding exception %s'
                     % exc)
            exc_info = sys.exc_info()
            msg = 'unexpected error: %s' % exc
            interpart = bundlepart('error:abort', [('message', msg)],
                                   mandatory=False)
            interpart.id = 0
            yield _pack(_fpayloadsize, -1)
            for chunk in interpart.getchunks(ui=ui):
                yield chunk
            outdebug(ui, 'closing payload chunk')
            # abort current part payload
            yield _pack(_fpayloadsize, 0)
            raise exc_info[0], exc_info[1], exc_info[2]
        # end of payload
        outdebug(ui, 'closing payload chunk')
        yield _pack(_fpayloadsize, 0)
        self._generated = True

    def _payloadchunks(self):
        """yield chunks of a the part payload

        Exists to handle the different methods to provide data to a part."""
        # we only support fixed size data now.
        # This will be improved in the future.
        if util.safehasattr(self.data, 'next'):
            buff = util.chunkbuffer(self.data)
            chunk = buff.read(preferedchunksize)
            while chunk:
                yield chunk
                chunk = buff.read(preferedchunksize)
        elif len(self.data):
            yield self.data


flaginterrupt = -1

class interrupthandler(unpackermixin):
    """read one part and process it with restricted capability

    This allows to transmit exception raised on the producer size during part
    iteration while the consumer is reading a part.

    Part processed in this manner only have access to a ui object,"""

    def __init__(self, ui, fp):
        super(interrupthandler, self).__init__(fp)
        self.ui = ui

    def _readpartheader(self):
        """reads a part header size and return the bytes blob

        returns None if empty"""
        headersize = self._unpack(_fpartheadersize)[0]
        if headersize < 0:
            raise error.BundleValueError('negative part header size: %i'
                                         % headersize)
        indebug(self.ui, 'part header size: %i\n' % headersize)
        if headersize:
            return self._readexact(headersize)
        return None

    def __call__(self):

        self.ui.debug('bundle2-input-stream-interrupt:'
                      ' opening out of band context\n')
        indebug(self.ui, 'bundle2 stream interruption, looking for a part.')
        headerblock = self._readpartheader()
        if headerblock is None:
            indebug(self.ui, 'no part found during interruption.')
            return
        part = unbundlepart(self.ui, headerblock, self._fp)
        op = interruptoperation(self.ui)
        _processpart(op, part)
        self.ui.debug('bundle2-input-stream-interrupt:'
                      ' closing out of band context\n')

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
        raise RuntimeError('no repo access from stream interruption')

    def gettransaction(self):
        raise TransactionUnavailable('no repo access from stream interruption')

class unbundlepart(unpackermixin):
    """a bundle part read from a bundle"""

    def __init__(self, ui, header, fp):
        super(unbundlepart, self).__init__(fp)
        self.ui = ui
        # unbundle state attr
        self._headerdata = header
        self._headeroffset = 0
        self._initialized = False
        self.consumed = False
        # part data
        self.id = None
        self.type = None
        self.mandatoryparams = None
        self.advisoryparams = None
        self.params = None
        self.mandatorykeys = ()
        self._payloadstream = None
        self._readheader()
        self._mandatory = None
        self._chunkindex = [] #(payload, file) position tuples for chunk starts
        self._pos = 0

    def _fromheader(self, size):
        """return the next <size> byte from the header"""
        offset = self._headeroffset
        data = self._headerdata[offset:(offset + size)]
        self._headeroffset = offset + size
        return data

    def _unpackheader(self, format):
        """read given format from header

        This automatically compute the size of the format to read."""
        data = self._fromheader(struct.calcsize(format))
        return _unpack(format, data)

    def _initparams(self, mandatoryparams, advisoryparams):
        """internal function to setup all logic related parameters"""
        # make it read only to prevent people touching it by mistake.
        self.mandatoryparams = tuple(mandatoryparams)
        self.advisoryparams  = tuple(advisoryparams)
        # user friendly UI
        self.params = dict(self.mandatoryparams)
        self.params.update(dict(self.advisoryparams))
        self.mandatorykeys = frozenset(p[0] for p in mandatoryparams)

    def _payloadchunks(self, chunknum=0):
        '''seek to specified chunk and start yielding data'''
        if len(self._chunkindex) == 0:
            assert chunknum == 0, 'Must start with chunk 0'
            self._chunkindex.append((0, super(unbundlepart, self).tell()))
        else:
            assert chunknum < len(self._chunkindex), \
                   'Unknown chunk %d' % chunknum
            super(unbundlepart, self).seek(self._chunkindex[chunknum][1])

        pos = self._chunkindex[chunknum][0]
        payloadsize = self._unpack(_fpayloadsize)[0]
        indebug(self.ui, 'payload chunk size: %i' % payloadsize)
        while payloadsize:
            if payloadsize == flaginterrupt:
                # interruption detection, the handler will now read a
                # single part and process it.
                interrupthandler(self.ui, self._fp)()
            elif payloadsize < 0:
                msg = 'negative payload chunk size: %i' %  payloadsize
                raise error.BundleValueError(msg)
            else:
                result = self._readexact(payloadsize)
                chunknum += 1
                pos += payloadsize
                if chunknum == len(self._chunkindex):
                    self._chunkindex.append((pos,
                                             super(unbundlepart, self).tell()))
                yield result
            payloadsize = self._unpack(_fpayloadsize)[0]
            indebug(self.ui, 'payload chunk size: %i' % payloadsize)

    def _findchunk(self, pos):
        '''for a given payload position, return a chunk number and offset'''
        for chunk, (ppos, fpos) in enumerate(self._chunkindex):
            if ppos == pos:
                return chunk, 0
            elif ppos > pos:
                return chunk - 1, pos - self._chunkindex[chunk - 1][0]
        raise ValueError('Unknown chunk')

    def _readheader(self):
        """read the header and setup the object"""
        typesize = self._unpackheader(_fparttypesize)[0]
        self.type = self._fromheader(typesize)
        indebug(self.ui, 'part type: "%s"' % self.type)
        self.id = self._unpackheader(_fpartid)[0]
        indebug(self.ui, 'part id: "%s"' % self.id)
        # extract mandatory bit from type
        self.mandatory = (self.type != self.type.lower())
        self.type = self.type.lower()
        ## reading parameters
        # param count
        mancount, advcount = self._unpackheader(_fpartparamcount)
        indebug(self.ui, 'part parameters: %i' % (mancount + advcount))
        # param size
        fparamsizes = _makefpartparamsizes(mancount + advcount)
        paramsizes = self._unpackheader(fparamsizes)
        # make it a list of couple again
        paramsizes = zip(paramsizes[::2], paramsizes[1::2])
        # split mandatory from advisory
        mansizes = paramsizes[:mancount]
        advsizes = paramsizes[mancount:]
        # retrieve param value
        manparams = []
        for key, value in mansizes:
            manparams.append((self._fromheader(key), self._fromheader(value)))
        advparams = []
        for key, value in advsizes:
            advparams.append((self._fromheader(key), self._fromheader(value)))
        self._initparams(manparams, advparams)
        ## part payload
        self._payloadstream = util.chunkbuffer(self._payloadchunks())
        # we read the data, tell it
        self._initialized = True

    def read(self, size=None):
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
                self.ui.debug('bundle2-input-part: total payload size %i\n'
                              % self._pos)
            self.consumed = True
        return data

    def tell(self):
        return self._pos

    def seek(self, offset, whence=0):
        if whence == 0:
            newpos = offset
        elif whence == 1:
            newpos = self._pos + offset
        elif whence == 2:
            if not self.consumed:
                self.read()
            newpos = self._chunkindex[-1][0] - offset
        else:
            raise ValueError('Unknown whence value: %r' % (whence,))

        if newpos > self._chunkindex[-1][0] and not self.consumed:
            self.read()
        if not 0 <= newpos <= self._chunkindex[-1][0]:
            raise ValueError('Offset out of range')

        if self._pos != newpos:
            chunk, internaloffset = self._findchunk(newpos)
            self._payloadstream = util.chunkbuffer(self._payloadchunks(chunk))
            adjust = self.read(internaloffset)
            if len(adjust) != internaloffset:
                raise util.Abort(_('Seek failed\n'))
            self._pos = newpos

# These are only the static capabilities.
# Check the 'getrepocaps' function for the rest.
capabilities = {'HG20': (),
                'error': ('abort', 'unsupportedcontent', 'pushraced',
                          'pushkey'),
                'listkeys': (),
                'pushkey': (),
                'digests': tuple(sorted(util.DIGESTS.keys())),
                'remote-changegroup': ('http', 'https'),
                'hgtagsfnodes': (),
               }

def getrepocaps(repo, allowpushback=False):
    """return the bundle2 capabilities for a given repo

    Exists to allow extensions (like evolution) to mutate the capabilities.
    """
    caps = capabilities.copy()
    caps['changegroup'] = tuple(sorted(changegroup.packermap.keys()))
    if obsolete.isenabled(repo, obsolete.exchangeopt):
        supportedformat = tuple('V%i' % v for v in obsolete.formats)
        caps['obsmarkers'] = supportedformat
    if allowpushback:
        caps['pushback'] = ()
    return caps

def bundle2caps(remote):
    """return the bundle capabilities of a peer as dict"""
    raw = remote.capable('bundle2')
    if not raw and raw != '':
        return {}
    capsblob = urllib.unquote(remote.capable('bundle2'))
    return decodecaps(capsblob)

def obsmarkersversion(caps):
    """extract the list of supported obsmarkers versions from a bundle2caps dict
    """
    obscaps = caps.get('obsmarkers', ())
    return [int(c[1:]) for c in obscaps if c.startswith('V')]

@parthandler('changegroup', ('version',))
def handlechangegroup(op, inpart):
    """apply a changegroup part on the repo

    This is a very early implementation that will massive rework before being
    inflicted to any end-user.
    """
    # Make sure we trigger a transaction creation
    #
    # The addchangegroup function will get a transaction object by itself, but
    # we need to make sure we trigger the creation of a transaction object used
    # for the whole processing scope.
    op.gettransaction()
    unpackerversion = inpart.params.get('version', '01')
    # We should raise an appropriate exception here
    unpacker = changegroup.packermap[unpackerversion][1]
    cg = unpacker(inpart, 'UN')
    # the source and url passed here are overwritten by the one contained in
    # the transaction.hookargs argument. So 'bundle2' is a placeholder
    ret = changegroup.addchangegroup(op.repo, cg, 'bundle2', 'bundle2')
    op.records.add('changegroup', {'return': ret})
    if op.reply is not None:
        # This is definitely not the final form of this
        # return. But one need to start somewhere.
        part = op.reply.newpart('reply:changegroup', mandatory=False)
        part.addparam('in-reply-to', str(inpart.id), mandatory=False)
        part.addparam('return', '%i' % ret, mandatory=False)
    assert not inpart.read()

_remotechangegroupparams = tuple(['url', 'size', 'digests'] +
    ['digest:%s' % k for k in util.DIGESTS.keys()])
@parthandler('remote-changegroup', _remotechangegroupparams)
def handleremotechangegroup(op, inpart):
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
        raw_url = inpart.params['url']
    except KeyError:
        raise util.Abort(_('remote-changegroup: missing "%s" param') % 'url')
    parsed_url = util.url(raw_url)
    if parsed_url.scheme not in capabilities['remote-changegroup']:
        raise util.Abort(_('remote-changegroup does not support %s urls') %
            parsed_url.scheme)

    try:
        size = int(inpart.params['size'])
    except ValueError:
        raise util.Abort(_('remote-changegroup: invalid value for param "%s"')
            % 'size')
    except KeyError:
        raise util.Abort(_('remote-changegroup: missing "%s" param') % 'size')

    digests = {}
    for typ in inpart.params.get('digests', '').split():
        param = 'digest:%s' % typ
        try:
            value = inpart.params[param]
        except KeyError:
            raise util.Abort(_('remote-changegroup: missing "%s" param') %
                param)
        digests[typ] = value

    real_part = util.digestchecker(url.open(op.ui, raw_url), size, digests)

    # Make sure we trigger a transaction creation
    #
    # The addchangegroup function will get a transaction object by itself, but
    # we need to make sure we trigger the creation of a transaction object used
    # for the whole processing scope.
    op.gettransaction()
    import exchange
    cg = exchange.readbundle(op.repo.ui, real_part, raw_url)
    if not isinstance(cg, changegroup.cg1unpacker):
        raise util.Abort(_('%s: not a bundle version 1.0') %
            util.hidepassword(raw_url))
    ret = changegroup.addchangegroup(op.repo, cg, 'bundle2', 'bundle2')
    op.records.add('changegroup', {'return': ret})
    if op.reply is not None:
        # This is definitely not the final form of this
        # return. But one need to start somewhere.
        part = op.reply.newpart('reply:changegroup')
        part.addparam('in-reply-to', str(inpart.id), mandatory=False)
        part.addparam('return', '%i' % ret, mandatory=False)
    try:
        real_part.validate()
    except util.Abort, e:
        raise util.Abort(_('bundle at %s is corrupted:\n%s') %
            (util.hidepassword(raw_url), str(e)))
    assert not inpart.read()

@parthandler('reply:changegroup', ('return', 'in-reply-to'))
def handlereplychangegroup(op, inpart):
    ret = int(inpart.params['return'])
    replyto = int(inpart.params['in-reply-to'])
    op.records.add('changegroup', {'return': ret}, replyto)

@parthandler('check:heads')
def handlecheckheads(op, inpart):
    """check that head of the repo did not change

    This is used to detect a push race when using unbundle.
    This replaces the "heads" argument of unbundle."""
    h = inpart.read(20)
    heads = []
    while len(h) == 20:
        heads.append(h)
        h = inpart.read(20)
    assert not h
    if heads != op.repo.heads():
        raise error.PushRaced('repository changed while pushing - '
                              'please try again')

@parthandler('output')
def handleoutput(op, inpart):
    """forward output captured on the server to the client"""
    for line in inpart.read().splitlines():
        op.ui.status(('remote: %s\n' % line))

@parthandler('replycaps')
def handlereplycaps(op, inpart):
    """Notify that a reply bundle should be created

    The payload contains the capabilities information for the reply"""
    caps = decodecaps(inpart.read())
    if op.reply is None:
        op.reply = bundle20(op.ui, caps)

@parthandler('error:abort', ('message', 'hint'))
def handleerrorabort(op, inpart):
    """Used to transmit abort error over the wire"""
    raise util.Abort(inpart.params['message'], hint=inpart.params.get('hint'))

@parthandler('error:pushkey', ('namespace', 'key', 'new', 'old', 'ret',
                               'in-reply-to'))
def handleerrorpushkey(op, inpart):
    """Used to transmit failure of a mandatory pushkey over the wire"""
    kwargs = {}
    for name in ('namespace', 'key', 'new', 'old', 'ret'):
        value = inpart.params.get(name)
        if value is not None:
            kwargs[name] = value
    raise error.PushkeyFailed(inpart.params['in-reply-to'], **kwargs)

@parthandler('error:unsupportedcontent', ('parttype', 'params'))
def handleerrorunsupportedcontent(op, inpart):
    """Used to transmit unknown content error over the wire"""
    kwargs = {}
    parttype = inpart.params.get('parttype')
    if parttype is not None:
        kwargs['parttype'] = parttype
    params = inpart.params.get('params')
    if params is not None:
        kwargs['params'] = params.split('\0')

    raise error.UnsupportedPartError(**kwargs)

@parthandler('error:pushraced', ('message',))
def handleerrorpushraced(op, inpart):
    """Used to transmit push race error over the wire"""
    raise error.ResponseError(_('push failed:'), inpart.params['message'])

@parthandler('listkeys', ('namespace',))
def handlelistkeys(op, inpart):
    """retrieve pushkey namespace content stored in a bundle2"""
    namespace = inpart.params['namespace']
    r = pushkey.decodekeys(inpart.read())
    op.records.add('listkeys', (namespace, r))

@parthandler('pushkey', ('namespace', 'key', 'old', 'new'))
def handlepushkey(op, inpart):
    """process a pushkey request"""
    dec = pushkey.decode
    namespace = dec(inpart.params['namespace'])
    key = dec(inpart.params['key'])
    old = dec(inpart.params['old'])
    new = dec(inpart.params['new'])
    ret = op.repo.pushkey(namespace, key, old, new)
    record = {'namespace': namespace,
              'key': key,
              'old': old,
              'new': new}
    op.records.add('pushkey', record)
    if op.reply is not None:
        rpart = op.reply.newpart('reply:pushkey')
        rpart.addparam('in-reply-to', str(inpart.id), mandatory=False)
        rpart.addparam('return', '%i' % ret, mandatory=False)
    if inpart.mandatory and not ret:
        kwargs = {}
        for key in ('namespace', 'key', 'new', 'old', 'ret'):
            if key in inpart.params:
                kwargs[key] = inpart.params[key]
        raise error.PushkeyFailed(partid=str(inpart.id), **kwargs)

@parthandler('reply:pushkey', ('return', 'in-reply-to'))
def handlepushkeyreply(op, inpart):
    """retrieve the result of a pushkey request"""
    ret = int(inpart.params['return'])
    partid = int(inpart.params['in-reply-to'])
    op.records.add('pushkey', {'return': ret}, partid)

@parthandler('obsmarkers')
def handleobsmarker(op, inpart):
    """add a stream of obsmarkers to the repo"""
    tr = op.gettransaction()
    markerdata = inpart.read()
    if op.ui.config('experimental', 'obsmarkers-exchange-debug', False):
        op.ui.write(('obsmarker-exchange: %i bytes received\n')
                    % len(markerdata))
    new = op.repo.obsstore.mergemarkers(tr, markerdata)
    if new:
        op.repo.ui.status(_('%i new obsolescence markers\n') % new)
    op.records.add('obsmarkers', {'new': new})
    if op.reply is not None:
        rpart = op.reply.newpart('reply:obsmarkers')
        rpart.addparam('in-reply-to', str(inpart.id), mandatory=False)
        rpart.addparam('new', '%i' % new, mandatory=False)


@parthandler('reply:obsmarkers', ('new', 'in-reply-to'))
def handleobsmarkerreply(op, inpart):
    """retrieve the result of a pushkey request"""
    ret = int(inpart.params['new'])
    partid = int(inpart.params['in-reply-to'])
    op.records.add('obsmarkers', {'new': ret}, partid)

@parthandler('hgtagsfnodes')
def handlehgtagsfnodes(op, inpart):
    """Applies .hgtags fnodes cache entries to the local repo.

    Payload is pairs of 20 byte changeset nodes and filenodes.
    """
    cache = tags.hgtagsfnodescache(op.repo.unfiltered())

    count = 0
    while True:
        node = inpart.read(20)
        fnode = inpart.read(20)
        if len(node) < 20 or len(fnode) < 20:
            op.ui.debug('received incomplete .hgtags fnodes data, ignoring\n')
            break
        cache.setfnode(node, fnode)
        count += 1

    cache.write()
    op.ui.debug('applied %i hgtags fnodes cache entries\n' % count)
