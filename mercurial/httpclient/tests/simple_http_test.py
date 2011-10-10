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
import socket
import unittest

import http

# relative import to ease embedding the library
import util


class SimpleHttpTest(util.HttpTestBase, unittest.TestCase):

    def _run_simple_test(self, host, server_data, expected_req, expected_data):
        con = http.HTTPConnection(host)
        con._connect()
        con.sock.data = server_data
        con.request('GET', '/')

        self.assertStringEqual(expected_req, con.sock.sent)
        self.assertEqual(expected_data, con.getresponse().read())

    def test_broken_data_obj(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        self.assertRaises(http.BadRequestData,
                          con.request, 'POST', '/', body=1)

    def test_no_keepalive_http_1_0(self):
        expected_request_one = """GET /remote/.hg/requires HTTP/1.1
Host: localhost:9999
range: bytes=0-
accept-encoding: identity
accept: application/mercurial-0.1
user-agent: mercurial/proto-1.0

""".replace('\n', '\r\n')
        expected_response_headers = """HTTP/1.0 200 OK
Server: SimpleHTTP/0.6 Python/2.6.1
Date: Sun, 01 May 2011 13:56:57 GMT
Content-type: application/octet-stream
Content-Length: 33
Last-Modified: Sun, 01 May 2011 13:56:56 GMT

""".replace('\n', '\r\n')
        expected_response_body = """revlogv1
store
fncache
dotencode
"""
        con = http.HTTPConnection('localhost:9999')
        con._connect()
        con.sock.data = [expected_response_headers, expected_response_body]
        con.request('GET', '/remote/.hg/requires',
                    headers={'accept-encoding': 'identity',
                             'range': 'bytes=0-',
                             'accept': 'application/mercurial-0.1',
                             'user-agent': 'mercurial/proto-1.0',
                             })
        self.assertStringEqual(expected_request_one, con.sock.sent)
        self.assertEqual(con.sock.closed, False)
        self.assertNotEqual(con.sock.data, [])
        self.assert_(con.busy())
        resp = con.getresponse()
        self.assertStringEqual(resp.read(), expected_response_body)
        self.failIf(con.busy())
        self.assertEqual(con.sock, None)
        self.assertEqual(resp.sock.data, [])
        self.assert_(resp.sock.closed)

    def test_multiline_header(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        con.sock.data = ['HTTP/1.1 200 OK\r\n',
                         'Server: BogusServer 1.0\r\n',
                         'Multiline: Value\r\n',
                         '  Rest of value\r\n',
                         'Content-Length: 10\r\n',
                         '\r\n'
                         '1234567890'
                         ]
        con.request('GET', '/')

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)
        resp = con.getresponse()
        self.assertEqual('1234567890', resp.read())
        self.assertEqual(['Value\n Rest of value'],
                         resp.headers.getheaders('multiline'))
        # Socket should not be closed
        self.assertEqual(resp.sock.closed, False)
        self.assertEqual(con.sock.closed, False)

    def testSimpleRequest(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        con.sock.data = ['HTTP/1.1 200 OK\r\n',
                         'Server: BogusServer 1.0\r\n',
                         'MultiHeader: Value\r\n'
                         'MultiHeader: Other Value\r\n'
                         'MultiHeader: One More!\r\n'
                         'Content-Length: 10\r\n',
                         '\r\n'
                         '1234567890'
                         ]
        con.request('GET', '/')

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)
        resp = con.getresponse()
        self.assertEqual('1234567890', resp.read())
        self.assertEqual(['Value', 'Other Value', 'One More!'],
                         resp.headers.getheaders('multiheader'))
        self.assertEqual(['BogusServer 1.0'],
                         resp.headers.getheaders('server'))

    def testHeaderlessResponse(self):
        con = http.HTTPConnection('1.2.3.4', use_ssl=False)
        con._connect()
        con.sock.data = ['HTTP/1.1 200 OK\r\n',
                         '\r\n'
                         '1234567890'
                         ]
        con.request('GET', '/')

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)
        resp = con.getresponse()
        self.assertEqual('1234567890', resp.read())
        self.assertEqual({}, dict(resp.headers))
        self.assertEqual(resp.status, 200)

    def testReadline(self):
        con = http.HTTPConnection('1.2.3.4')
        con._connect()
        # make sure it trickles in one byte at a time
        # so that we touch all the cases in readline
        con.sock.data = list(''.join(
            ['HTTP/1.1 200 OK\r\n',
             'Server: BogusServer 1.0\r\n',
             'Connection: Close\r\n',
             '\r\n'
             '1\n2\nabcdefg\n4\n5']))

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        con.request('GET', '/')
        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)
        r = con.getresponse()
        for expected in ['1\n', '2\n', 'abcdefg\n', '4\n', '5']:
            actual = r.readline()
            self.assertEqual(expected, actual,
                             'Expected %r, got %r' % (expected, actual))

    def testIPv6(self):
        self._run_simple_test('[::1]:8221',
                        ['HTTP/1.1 200 OK\r\n',
                         'Server: BogusServer 1.0\r\n',
                         'Content-Length: 10',
                         '\r\n\r\n'
                         '1234567890'],
                        ('GET / HTTP/1.1\r\n'
                         'Host: [::1]:8221\r\n'
                         'accept-encoding: identity\r\n\r\n'),
                        '1234567890')
        self._run_simple_test('::2',
                        ['HTTP/1.1 200 OK\r\n',
                         'Server: BogusServer 1.0\r\n',
                         'Content-Length: 10',
                         '\r\n\r\n'
                         '1234567890'],
                        ('GET / HTTP/1.1\r\n'
                         'Host: ::2\r\n'
                         'accept-encoding: identity\r\n\r\n'),
                        '1234567890')
        self._run_simple_test('[::3]:443',
                        ['HTTP/1.1 200 OK\r\n',
                         'Server: BogusServer 1.0\r\n',
                         'Content-Length: 10',
                         '\r\n\r\n'
                         '1234567890'],
                        ('GET / HTTP/1.1\r\n'
                         'Host: ::3\r\n'
                         'accept-encoding: identity\r\n\r\n'),
                        '1234567890')

    def testEarlyContinueResponse(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.data = ['HTTP/1.1 403 Forbidden\r\n',
                         'Server: BogusServer 1.0\r\n',
                         'Content-Length: 18',
                         '\r\n\r\n'
                         "You can't do that."]
        expected_req = self.doPost(con, expect_body=False)
        self.assertEqual(('1.2.3.4', 80), sock.sa)
        self.assertStringEqual(expected_req, sock.sent)
        self.assertEqual("You can't do that.", con.getresponse().read())
        self.assertEqual(sock.closed, True)

    def testDeniedAfterContinueTimeoutExpires(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.data = ['HTTP/1.1 403 Forbidden\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'Content-Length: 18\r\n',
                     'Connection: close',
                     '\r\n\r\n'
                     "You can't do that."]
        sock.read_wait_sentinel = 'Dear server, send response!'
        sock.close_on_empty = True
        # send enough data out that we'll chunk it into multiple
        # blocks and the socket will close before we can send the
        # whole request.
        post_body = ('This is some POST data\n' * 1024 * 32 +
                     'Dear server, send response!\n' +
                     'This is some POST data\n' * 1024 * 32)
        expected_req = self.doPost(con, expect_body=False,
                                   body_to_send=post_body)
        self.assertEqual(('1.2.3.4', 80), sock.sa)
        self.assert_('POST data\n' in sock.sent)
        self.assert_('Dear server, send response!\n' in sock.sent)
        # We expect not all of our data was sent.
        self.assertNotEqual(sock.sent, expected_req)
        self.assertEqual("You can't do that.", con.getresponse().read())
        self.assertEqual(sock.closed, True)

    def testPostData(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.read_wait_sentinel = 'POST data'
        sock.early_data = ['HTTP/1.1 100 Co', 'ntinue\r\n\r\n']
        sock.data = ['HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'Content-Length: 16',
                     '\r\n\r\n',
                     "You can do that."]
        expected_req = self.doPost(con, expect_body=True)
        self.assertEqual(('1.2.3.4', 80), sock.sa)
        self.assertEqual(expected_req, sock.sent)
        self.assertEqual("You can do that.", con.getresponse().read())
        self.assertEqual(sock.closed, False)

    def testServerWithoutContinue(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.read_wait_sentinel = 'POST data'
        sock.data = ['HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'Content-Length: 16',
                     '\r\n\r\n',
                     "You can do that."]
        expected_req = self.doPost(con, expect_body=True)
        self.assertEqual(('1.2.3.4', 80), sock.sa)
        self.assertEqual(expected_req, sock.sent)
        self.assertEqual("You can do that.", con.getresponse().read())
        self.assertEqual(sock.closed, False)

    def testServerWithSlowContinue(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        sock = con.sock
        sock.read_wait_sentinel = 'POST data'
        sock.data = ['HTTP/1.1 100 ', 'Continue\r\n\r\n',
                     'HTTP/1.1 200 OK\r\n',
                     'Server: BogusServer 1.0\r\n',
                     'Content-Length: 16',
                     '\r\n\r\n',
                     "You can do that."]
        expected_req = self.doPost(con, expect_body=True)
        self.assertEqual(('1.2.3.4', 80), sock.sa)
        self.assertEqual(expected_req, sock.sent)
        resp = con.getresponse()
        self.assertEqual("You can do that.", resp.read())
        self.assertEqual(200, resp.status)
        self.assertEqual(sock.closed, False)

    def testSlowConnection(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        # simulate one byte arriving at a time, to check for various
        # corner cases
        con.sock.data = list('HTTP/1.1 200 OK\r\n'
                             'Server: BogusServer 1.0\r\n'
                             'Content-Length: 10'
                             '\r\n\r\n'
                             '1234567890')
        con.request('GET', '/')

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)
        self.assertEqual('1234567890', con.getresponse().read())

    def testTimeout(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        con.sock.data = []
        con.request('GET', '/')
        self.assertRaises(http.HTTPTimeoutException,
                          con.getresponse)

        expected_req = ('GET / HTTP/1.1\r\n'
                        'Host: 1.2.3.4\r\n'
                        'accept-encoding: identity\r\n\r\n')

        self.assertEqual(('1.2.3.4', 80), con.sock.sa)
        self.assertEqual(expected_req, con.sock.sent)

    def test_conn_keep_alive_but_server_close_anyway(self):
        sockets = []
        def closingsocket(*args, **kwargs):
            s = util.MockSocket(*args, **kwargs)
            sockets.append(s)
            s.data = ['HTTP/1.1 200 OK\r\n',
                      'Server: BogusServer 1.0\r\n',
                      'Connection: Keep-Alive\r\n',
                      'Content-Length: 16',
                      '\r\n\r\n',
                      'You can do that.']
            s.close_on_empty = True
            return s

        socket.socket = closingsocket
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        con.request('GET', '/')
        r1 = con.getresponse()
        r1.read()
        self.assertFalse(con.sock.closed)
        self.assert_(con.sock.remote_closed)
        con.request('GET', '/')
        self.assertEqual(2, len(sockets))

    def test_server_closes_before_end_of_body(self):
        con = http.HTTPConnection('1.2.3.4:80')
        con._connect()
        s = con.sock
        s.data = ['HTTP/1.1 200 OK\r\n',
                  'Server: BogusServer 1.0\r\n',
                  'Connection: Keep-Alive\r\n',
                  'Content-Length: 16',
                  '\r\n\r\n',
                  'You can '] # Note: this is shorter than content-length
        s.close_on_empty = True
        con.request('GET', '/')
        r1 = con.getresponse()
        self.assertRaises(http.HTTPRemoteClosedError, r1.read)

    def test_no_response_raises_response_not_ready(self):
        con = http.HTTPConnection('foo')
        self.assertRaises(http.httplib.ResponseNotReady, con.getresponse)
# no-check-code
