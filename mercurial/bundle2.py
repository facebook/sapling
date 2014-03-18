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

The current implementation is limited to empty bundle.

Details on the Binary format
============================

All numbers are unsigned and big endian.

stream level parameters
------------------------

Binary format is as follow

:params size: (16 bits integer)

  The total number of Bytes used by the parameters

  Currently force to 0.

:params value: arbitrary number of Bytes

  A blob of `params size` containing the serialized version of all stream level
  parameters.

  Currently always empty.


Payload part
------------------------

Binary format is as follow

:header size: (16 bits inter)

  The total number of Bytes used by the part headers. When the header is empty
  (size = 0) this is interpreted as the end of stream marker.

  Currently forced to 0 in the current state of the implementation
"""

import util
import changegroup


_magicstring = 'HG20'

class bundle20(object):
    """represent an outgoing bundle2 container

    People will eventually be able to add param and parts to this object and
    generated a stream from it."""

    def __init__(self):
        self._params = []
        self._parts = []

    def getchunks(self):
        yield _magicstring
        # no support for any param yet
        # to be obviously fixed soon.
        assert not self._params
        yield '\0\0'
        # no support for parts
        # to be obviously fixed soon.
        assert not self._parts
        yield '\0\0'

class unbundle20(object):
    """interpret a bundle2 stream

    (this will eventually yield parts)"""

    def __init__(self, fp):
        # assume the magic string is ok and drop it
        # to be obviously fixed soon.
        self._fp = fp
        self._readexact(4)

    def _unpack(self, format):
        """unpack this struct format from the stream"""
        data = self._readexact(struct.calcsize(format))
        return _unpack(format, data)

    def _readexact(self, size):
        """read exactly <size> bytes from the stream"""
        return changegroup.readexactly(self._fp, size)

    @util.propertycache
    def params(self):
        """dictionnary of stream level parameters"""
        paramsize = self._readexact(2)
        assert paramsize == '\0\0'
        return {}

    def __iter__(self):
        """yield all parts contained in the stream"""
        # make sure param have been loaded
        self.params
        part = self._readpart()
        while part is not None:
            yield part
            part = self._readpart()

    def _readpart(self):
        """return None when an end of stream markers is reach"""
        headersize = self._readexact(2)
        assert headersize == '\0\0'
        return None



