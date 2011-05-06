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
"""Tests against malformed responses.

Server implementations that respond with only LF instead of CRLF have
been observed. Checking against ones that use only CR is a hedge
against that potential insanit.y
"""
import unittest

import http

# relative import to ease embedding the library
import util


class SimpleHttpTest(util.HttpTestBase, unittest.TestCase):

    def bogusEOL(self, eol):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        con.sock.data = ['HTTP/1.1 200 OK%s' % eol,
                         'Server: BogusServer 1.0%s' % eol,
                         'Content-Length: 10',
                         eol * 2,
                         '1234567890']
        con.request('GET', '/')

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)
        self.assertEqual('1234567890', con.getresponse().read())

    def testOnlyLinefeed(self):
        self.bogusEOL('\n')

    def testOnlyCarriageReturn(self):
        self.bogusEOL('\r')
# no-check-code
