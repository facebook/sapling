# Copyright 2011, Google Inc.
# All rights reserved.
#
# Redistribution and use in source and binary forms, with or without
# modification, are permitted provided that the following conditions are
# met:
#
#     * Redistributions of source code must retain the above copyright
# notice, this list of conditions and the following disclaimer.
#     * Redistributions in binary form must reproduce the above
# copyright notice, this list of conditions and the following disclaimer
# in the documentation and/or other materials provided with the
# distribution.
#     * Neither the name of Google Inc. nor the names of its
# contributors may be used to endorse or promote products derived from
# this software without specific prior written permission.

# THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
# "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
# LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
# A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
# OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
# SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
# LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
# DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
# THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
# (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
# OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
"""Reader objects to abstract out different body response types.

This module is package-private. It is not expected that these will
have any clients outside of httpplus.
"""

import httplib
import itertools
import logging

logger = logging.getLogger(__name__)


class ReadNotReady(Exception):
    """Raised when read() is attempted but not enough data is loaded."""


class HTTPRemoteClosedError(httplib.HTTPException):
    """The server closed the remote socket in the middle of a response."""


class AbstractReader(object):
    """Abstract base class for response readers.

    Subclasses must implement _load, and should implement _close if
    it's not an error for the server to close their socket without
    some termination condition being detected during _load.
    """
    def __init__(self):
        self._finished = False
        self._done_chunks = []
        self.available_data = 0

    def addchunk(self, data):
        self._done_chunks.append(data)
        self.available_data += len(data)

    def pushchunk(self, data):
        self._done_chunks.insert(0, data)
        self.available_data += len(data)

    def popchunk(self):
        b = self._done_chunks.pop(0)
        self.available_data -= len(b)

        return b

    def done(self):
        return self._finished

    def read(self, amt):
        if self.available_data < amt and not self._finished:
            raise ReadNotReady()
        need = [amt]
        def pred(s):
            needed = need[0] > 0
            need[0] -= len(s)
            return needed
        blocks = list(itertools.takewhile(pred, self._done_chunks))
        self._done_chunks = self._done_chunks[len(blocks):]
        over_read = sum(map(len, blocks)) - amt
        if over_read > 0 and blocks:
            logger.debug('need to reinsert %d data into done chunks', over_read)
            last = blocks[-1]
            blocks[-1], reinsert = last[:-over_read], last[-over_read:]
            self._done_chunks.insert(0, reinsert)
        result = ''.join(blocks)
        assert len(result) == amt or (self._finished and len(result) < amt)
        self.available_data -= amt
        return result

    def _load(self, data): # pragma: no cover
        """Subclasses must implement this.

        As data is available to be read out of this object, it should
        be placed into the _done_chunks list. Subclasses should not
        rely on data remaining in _done_chunks forever, as it may be
        reaped if the client is parsing data as it comes in.
        """
        raise NotImplementedError

    def _close(self):
        """Default implementation of close.

        The default implementation assumes that the reader will mark
        the response as finished on the _finished attribute once the
        entire response body has been read. In the event that this is
        not true, the subclass should override the implementation of
        close (for example, close-is-end responses have to set
        self._finished in the close handler.)
        """
        if not self._finished:
            raise HTTPRemoteClosedError(
                'server appears to have closed the socket mid-response')


class AbstractSimpleReader(AbstractReader):
    """Abstract base class for simple readers that require no response decoding.

    Examples of such responses are Connection: Close (close-is-end)
    and responses that specify a content length.
    """
    def _load(self, data):
        if data:
            assert not self._finished, (
                'tried to add data (%r) to a closed reader!' % data)
        logger.debug('%s read an additional %d data', self.name, len(data))
        self.addchunk(data)


class CloseIsEndReader(AbstractSimpleReader):
    """Reader for responses that specify Connection: Close for length."""
    name = 'close-is-end'

    def _close(self):
        logger.info('Marking close-is-end reader as closed.')
        self._finished = True


class ContentLengthReader(AbstractSimpleReader):
    """Reader for responses that specify an exact content length."""
    name = 'content-length'

    def __init__(self, amount):
        AbstractReader.__init__(self)
        self._amount = amount
        if amount == 0:
            self._finished = True
        self._amount_seen = 0

    def _load(self, data):
        AbstractSimpleReader._load(self, data)
        self._amount_seen += len(data)
        if self._amount_seen >= self._amount:
            self._finished = True
            logger.debug('content-length read complete')


class ChunkedReader(AbstractReader):
    """Reader for chunked transfer encoding responses."""
    def __init__(self, eol):
        AbstractReader.__init__(self)
        self._eol = eol
        self._leftover_skip_amt = 0
        self._leftover_data = ''

    def _load(self, data):
        assert not self._finished, 'tried to add data to a closed reader!'
        logger.debug('chunked read an additional %d data', len(data))
        position = 0
        if self._leftover_data:
            logger.debug('chunked reader trying to finish block from leftover data')
            # TODO: avoid this string concatenation if possible
            data = self._leftover_data + data
            position = self._leftover_skip_amt
            self._leftover_data = ''
            self._leftover_skip_amt = 0
        datalen = len(data)
        while position < datalen:
            split = data.find(self._eol, position)
            if split == -1:
                self._leftover_data = data
                self._leftover_skip_amt = position
                return
            amt = int(data[position:split], base=16)
            block_start = split + len(self._eol)
            # If the whole data chunk plus the eol trailer hasn't
            # loaded, we'll wait for the next load.
            if block_start + amt + len(self._eol) > len(data):
                self._leftover_data = data
                self._leftover_skip_amt = position
                return
            if amt == 0:
                self._finished = True
                logger.debug('closing chunked reader due to chunk of length 0')
                return
            self.addchunk(data[block_start:block_start + amt])
            position = block_start + amt + len(self._eol)
# no-check-code
