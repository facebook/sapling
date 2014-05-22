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

:params size: (16 bits integer)

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

:header size: (16 bits inter)

  The total number of Bytes used by the part headers. When the header is empty
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

    :parttype: alphanumerical part name

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

:payload:

    payload is a series of `<chunksize><chunkdata>`.

    `chunksize` is a 32 bits integer, `chunkdata` are plain bytes (as much as
    `chunksize` says)` The payload part is concluded by a zero size chunk.

    The current implementation always produces either zero or one chunk.
    This is an implementation limitation that will ultimately be lifted.

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

import util
import struct
import urllib
import string

import changegroup, error
from i18n import _

_pack = struct.pack
_unpack = struct.unpack

_magicstring = 'HG2X'

_fstreamparamsize = '>H'
_fpartheadersize = '>H'
_fparttypesize = '>B'
_fpartid = '>I'
_fpayloadsize = '>I'
_fpartparamcount = '>BB'

preferedchunksize = 4096

def _makefpartparamsizes(nbparams):
    """return a struct format to read part parameter sizes

    The number parameters is variable so we need to build that format
    dynamically.
    """
    return '>'+('BB'*nbparams)

class UnknownPartError(KeyError):
    """error raised when no handler is found for a Mandatory part"""
    pass

class ReadOnlyPartError(RuntimeError):
    """error raised when code tries to alter a part being generated"""
    pass

parthandlermapping = {}

def parthandler(parttype):
    """decorator that register a function as a bundle2 part handler

    eg::

        @parthandler('myparttype')
        def myparttypehandler(...):
            '''process a part of type "my part".'''
            ...
    """
    def _decorator(func):
        lparttype = parttype.lower() # enforce lower case matching.
        assert lparttype not in parthandlermapping
        parthandlermapping[lparttype] = func
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
        """get the subrecords that replies to a specific part"""
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

    def __init__(self, repo, transactiongetter):
        self.repo = repo
        self.ui = repo.ui
        self.records = unbundlerecords()
        self.gettransaction = transactiongetter
        self.reply = None

class TransactionUnavailable(RuntimeError):
    pass

def _notransaction():
    """default method to get a transaction while processing a bundle

    Raise an exception to highlight the fact that no transaction was expected
    to be created"""
    raise TransactionUnavailable()

def processbundle(repo, unbundler, transactiongetter=_notransaction):
    """This function process a bundle, apply effect to/from a repo

    It iterates over each part then searches for and uses the proper handling
    code to process the part. Parts are processed in order.

    This is very early version of this function that will be strongly reworked
    before final usage.

    Unknown Mandatory part will abort the process.
    """
    op = bundleoperation(repo, transactiongetter)
    # todo:
    # - replace this is a init function soon.
    # - exception catching
    unbundler.params
    iterparts = unbundler.iterparts()
    part = None
    try:
        for part in iterparts:
            parttype = part.type
            # part key are matched lower case
            key = parttype.lower()
            try:
                handler = parthandlermapping[key]
                op.ui.debug('found a handler for part %r\n' % parttype)
            except KeyError:
                if key != parttype: # mandatory parts
                    # todo:
                    # - use a more precise exception
                    raise UnknownPartError(key)
                op.ui.debug('ignoring unknown advisory part %r\n' % key)
                # consuming the part
                part.read()
                continue

            # handler is called outside the above try block so that we don't
            # risk catching KeyErrors from anything other than the
            # parthandlermapping lookup (any KeyError raised by handler()
            # itself represents a defect of a different variety).
            output = None
            if op.reply is not None:
                op.ui.pushbuffer(error=True)
                output = ''
            try:
                handler(op, part)
            finally:
                if output is not None:
                    output = op.ui.popbuffer()
            if output:
                op.reply.newpart('b2x:output',
                                 advisoryparams=[('in-reply-to',
                                                  str(part.id))],
                                 data=output)
            part.read()
    except Exception, exc:
        if part is not None:
            # consume the bundle content
            part.read()
        for part in iterparts:
            # consume the bundle content
            part.read()
        # Small hack to let caller code distinguish exceptions from bundle2
        # processing fron the ones from bundle1 processing. This is mostly
        # needed to handle different return codes to unbundle according to the
        # type of bundle. We should probably clean up or drop this return code
        # craziness in a future version.
        exc.duringunbundle2 = True
        raise
    return op

def decodecaps(blob):
    """decode a bundle2 caps bytes blob into a dictionnary

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

    def __init__(self, ui, capabilities=()):
        self.ui = ui
        self._params = []
        self._parts = []
        self.capabilities = dict(capabilities)

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
        self.ui.debug('start emission of %s stream\n' % _magicstring)
        yield _magicstring
        param = self._paramchunk()
        self.ui.debug('bundle parameter: %s\n' % param)
        yield _pack(_fstreamparamsize, len(param))
        if param:
            yield param

        self.ui.debug('start of parts\n')
        for part in self._parts:
            self.ui.debug('bundle part: "%s"\n' % part.type)
            for chunk in part.getchunks():
                yield chunk
        self.ui.debug('end of bundle\n')
        yield '\0\0'

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

class unpackermixin(object):
    """A mixin to extract bytes and struct data from a stream"""

    def __init__(self, fp):
        self._fp = fp

    def _unpack(self, format):
        """unpack this struct format from the stream"""
        data = self._readexact(struct.calcsize(format))
        return _unpack(format, data)

    def _readexact(self, size):
        """read exactly <size> bytes from the stream"""
        return changegroup.readexactly(self._fp, size)


class unbundle20(unpackermixin):
    """interpret a bundle2 stream

    This class is fed with a binary stream and yields parts through its
    `iterparts` methods."""

    def __init__(self, ui, fp, header=None):
        """If header is specified, we do not read it out of the stream."""
        self.ui = ui
        super(unbundle20, self).__init__(fp)
        if header is None:
            header = self._readexact(4)
            magic, version = header[0:2], header[2:4]
            if magic != 'HG':
                raise util.Abort(_('not a Mercurial bundle'))
            if version != '2X':
                raise util.Abort(_('unknown bundle version %s') % version)
        self.ui.debug('start processing of %s stream\n' % header)

    @util.propertycache
    def params(self):
        """dictionary of stream level parameters"""
        self.ui.debug('reading bundle2 stream parameters\n')
        params = {}
        paramssize = self._unpack(_fstreamparamsize)[0]
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
            self.ui.debug("ignoring unknown parameter %r\n" % name)
        else:
            raise KeyError(name)


    def iterparts(self):
        """yield all parts contained in the stream"""
        # make sure param have been loaded
        self.params
        self.ui.debug('start extraction of bundle2 parts\n')
        headerblock = self._readpartheader()
        while headerblock is not None:
            part = unbundlepart(self.ui, headerblock, self._fp)
            yield part
            headerblock = self._readpartheader()
        self.ui.debug('end of bundle2 stream\n')

    def _readpartheader(self):
        """reads a part header size and return the bytes blob

        returns None if empty"""
        headersize = self._unpack(_fpartheadersize)[0]
        self.ui.debug('part header size: %i\n' % headersize)
        if headersize:
            return self._readexact(headersize)
        return None


class bundlepart(object):
    """A bundle2 part contains application level payload

    The part `type` is used to route the part to the application level
    handler.

    The part payload is contained in ``part.data``. It could be raw bytes or a
    generator of byte chunks. The data attribute cannot be modified after the
    generation has begun.
    """

    def __init__(self, parttype, mandatoryparams=(), advisoryparams=(),
                 data=''):
        self.id = None
        self.type = parttype
        self._data = data
        self.mandatoryparams = mandatoryparams
        self.advisoryparams = advisoryparams
        # status of the part's generation:
        # - None: not started,
        # - False: currently generated,
        # - True: generation done.
        self._generated = None

    # methods used to defines the part content
    def __setdata(self, data):
        if self._generated is not None:
            raise ReadOnlyPartError('part is being generated')
        self._data = data
    def __getdata(self):
        return self._data
    data = property(__getdata, __setdata)

    # methods used to generates the bundle2 stream
    def getchunks(self):
        if self._generated is not None:
            raise RuntimeError('part can only be consumed once')
        self._generated = False
        #### header
        ## parttype
        header = [_pack(_fparttypesize, len(self.type)),
                  self.type, _pack(_fpartid, self.id),
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
        yield _pack(_fpartheadersize, len(headerchunk))
        yield headerchunk
        ## payload
        for chunk in self._payloadchunks():
            yield _pack(_fpayloadsize, len(chunk))
            yield chunk
        # end of payload
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
        self._payloadstream = None
        self._readheader()

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

    def _readheader(self):
        """read the header and setup the object"""
        typesize = self._unpackheader(_fparttypesize)[0]
        self.type = self._fromheader(typesize)
        self.ui.debug('part type: "%s"\n' % self.type)
        self.id = self._unpackheader(_fpartid)[0]
        self.ui.debug('part id: "%s"\n' % self.id)
        ## reading parameters
        # param count
        mancount, advcount = self._unpackheader(_fpartparamcount)
        self.ui.debug('part parameters: %i\n' % (mancount + advcount))
        # param size
        fparamsizes = _makefpartparamsizes(mancount + advcount)
        paramsizes = self._unpackheader(fparamsizes)
        # make it a list of couple again
        paramsizes = zip(paramsizes[::2], paramsizes[1::2])
        # split mandatory from advisory
        mansizes = paramsizes[:mancount]
        advsizes = paramsizes[mancount:]
        # retrive param value
        manparams = []
        for key, value in mansizes:
            manparams.append((self._fromheader(key), self._fromheader(value)))
        advparams = []
        for key, value in advsizes:
            advparams.append((self._fromheader(key), self._fromheader(value)))
        self.mandatoryparams = manparams
        self.advisoryparams  = advparams
        ## part payload
        def payloadchunks():
            payloadsize = self._unpack(_fpayloadsize)[0]
            self.ui.debug('payload chunk size: %i\n' % payloadsize)
            while payloadsize:
                yield self._readexact(payloadsize)
                payloadsize = self._unpack(_fpayloadsize)[0]
                self.ui.debug('payload chunk size: %i\n' % payloadsize)
        self._payloadstream = util.chunkbuffer(payloadchunks())
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
        if size is None or len(data) < size:
            self.consumed = True
        return data


@parthandler('b2x:changegroup')
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
    cg = changegroup.unbundle10(inpart, 'UN')
    ret = changegroup.addchangegroup(op.repo, cg, 'bundle2', 'bundle2')
    op.records.add('changegroup', {'return': ret})
    if op.reply is not None:
        # This is definitly not the final form of this
        # return. But one need to start somewhere.
        op.reply.newpart('b2x:reply:changegroup', (),
                         [('in-reply-to', str(inpart.id)),
                          ('return', '%i' % ret)])
    assert not inpart.read()

@parthandler('b2x:reply:changegroup')
def handlechangegroup(op, inpart):
    p = dict(inpart.advisoryparams)
    ret = int(p['return'])
    op.records.add('changegroup', {'return': ret}, int(p['in-reply-to']))

@parthandler('b2x:check:heads')
def handlechangegroup(op, inpart):
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

@parthandler('b2x:output')
def handleoutput(op, inpart):
    """forward output captured on the server to the client"""
    for line in inpart.read().splitlines():
        op.ui.write(('remote: %s\n' % line))

@parthandler('b2x:replycaps')
def handlereplycaps(op, inpart):
    """Notify that a reply bundle should be created

    The payload contains the capabilities information for the reply"""
    caps = decodecaps(inpart.read())
    if op.reply is None:
        op.reply = bundle20(op.ui, caps)

@parthandler('b2x:error:abort')
def handlereplycaps(op, inpart):
    """Used to transmit abort error over the wire"""
    manargs = dict(inpart.mandatoryparams)
    advargs = dict(inpart.advisoryparams)
    raise util.Abort(manargs['message'], hint=advargs.get('hint'))

@parthandler('b2x:error:unknownpart')
def handlereplycaps(op, inpart):
    """Used to transmit unknown part error over the wire"""
    manargs = dict(inpart.mandatoryparams)
    raise UnknownPartError(manargs['parttype'])

@parthandler('b2x:error:pushraced')
def handlereplycaps(op, inpart):
    """Used to transmit push race error over the wire"""
    manargs = dict(inpart.mandatoryparams)
    raise error.ResponseError(_('push failed:'), manargs['message'])
