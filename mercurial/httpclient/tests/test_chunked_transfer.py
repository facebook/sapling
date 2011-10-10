# Copyright 2010, Google Inc.
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
import cStringIO
import unittest

import http

# relative import to ease embedding the library
import util


def chunkedblock(x, eol='\r\n'):
    r"""Make a chunked transfer-encoding block.

    >>> chunkedblock('hi')
    '2\r\nhi\r\n'
    >>> chunkedblock('hi' * 10)
    '14\r\nhihihihihihihihihihi\r\n'
    >>> chunkedblock('hi', eol='\n')
    '2\nhi\n'
    """
    return ''.join((hex(len(x))[2:], eol, x, eol))


class ChunkedTransferTest(util.HttpTestBase, unittest.TestCase):
    def testChunkedUpload(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.read_wait_sentinel = '0\r\n\r\n'
        sock.data = ['HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'Content-Length: 6',
                     '\r\n\r\n',
                     "Thanks"]

        zz = 'zz\n'
        con.request('POST', '/', body=cStringIO.StringIO(
            (zz * (0x8010 / 3)) + 'end-of-body'))
        expected_req = ('POST / HTTP/1.1\r\n'
                        'transfer-encoding: chunked\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')
        expected_req += chunkedblock('zz\n' * (0x8000 / 3) + 'zz')
        expected_req += chunkedblock(
            '\n' + 'zz\n' * ((0x1b - len('end-of-body')) / 3) + 'end-of-body')
        expected_req += '0\r\n\r\n'
        self.assertEqual(('1.2.3.4', 80), sock.sa)
        self.assertStringEqual(expected_req, sock.sent)
        self.assertEqual("Thanks", con.getresponse().read())
        self.assertEqual(sock.closed, False)

    def testChunkedDownload(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.data = ['HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'transfer-encoding: chunked',
                     '\r\n\r\n',
                     chunkedblock('hi '),
                     chunkedblock('there'),
                     chunkedblock(''),
                     ]
        con.request('GET', '/')
        self.assertStringEqual('hi there', con.getresponse().read())

    def testChunkedDownloadBadEOL(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.data = ['HTTP/1.1 200 OK\n',
                     'Server: BogusServer 1.0\n',
                     'transfer-encoding: chunked',
                     '\n\n',
                     chunkedblock('hi ', eol='\n'),
                     chunkedblock('there', eol='\n'),
                     chunkedblock('', eol='\n'),
                     ]
        con.request('GET', '/')
        self.assertStringEqual('hi there', con.getresponse().read())

    def testChunkedDownloadPartialChunkBadEOL(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.data = ['HTTP/1.1 200 OK\n',
                     'Server: BogusServer 1.0\n',
                     'transfer-encoding: chunked',
                     '\n\n',
                     chunkedblock('hi ', eol='\n'),
                     ] + list(chunkedblock('there\n' * 5, eol='\n')) + [
                         chunkedblock('', eol='\n')]
        con.request('GET', '/')
        self.assertStringEqual('hi there\nthere\nthere\nthere\nthere\n',
                               con.getresponse().read())

    def testChunkedDownloadPartialChunk(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.data = ['HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'transfer-encoding: chunked',
                     '\r\n\r\n',
                     chunkedblock('hi '),
                     ] + list(chunkedblock('there\n' * 5)) + [chunkedblock('')]
        con.request('GET', '/')
        self.assertStringEqual('hi there\nthere\nthere\nthere\nthere\n',
                               con.getresponse().read())

    def testChunkedDownloadEarlyHangup(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        broken = chunkedblock('hi'*20)[:-1]
        sock.data = ['HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'transfer-encoding: chunked',
                     '\r\n\r\n',
                     broken,
                     ]
        sock.close_on_empty = True
        con.request('GET', '/')
        resp = con.getresponse()
        self.assertRaises(http.HTTPRemoteClosedError, resp.read)
# no-check-code
