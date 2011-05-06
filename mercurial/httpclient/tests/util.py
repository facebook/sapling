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
import difflib
import socket

import http


class MockSocket(object):
    """Mock non-blocking socket object.

    This is ONLY capable of mocking a nonblocking socket.

    Attributes:
      early_data: data to always send as soon as end of headers is seen
      data: a list of strings to return on recv(), with the
            assumption that the socket would block between each
            string in the list.
      read_wait_sentinel: data that must be written to the socket before
                          beginning the response.
      close_on_empty: If true, close the socket when it runs out of data
                      for the client.
    """
    def __init__(self, af, socktype, proto):
        self.af = af
        self.socktype = socktype
        self.proto = proto

        self.early_data = []
        self.data = []
        self.remote_closed = self.closed = False
        self.close_on_empty = False
        self.sent = ''
        self.read_wait_sentinel = http._END_HEADERS

    def close(self):
        self.closed = True

    def connect(self, sa):
        self.sa = sa

    def setblocking(self, timeout):
        assert timeout == 0

    def recv(self, amt=-1):
        if self.early_data:
            datalist = self.early_data
        elif not self.data:
            return ''
        else:
            datalist = self.data
        if amt == -1:
            return datalist.pop(0)
        data = datalist.pop(0)
        if len(data) > amt:
            datalist.insert(0, data[amt:])
        if not self.data and not self.early_data and self.close_on_empty:
            self.remote_closed = True
        return data[:amt]

    @property
    def ready_for_read(self):
        return ((self.early_data and http._END_HEADERS in self.sent)
                or (self.read_wait_sentinel in self.sent and self.data)
                or self.closed)

    def send(self, data):
        # this is a horrible mock, but nothing needs us to raise the
        # correct exception yet
        assert not self.closed, 'attempted to write to a closed socket'
        assert not self.remote_closed, ('attempted to write to a'
                                        ' socket closed by the server')
        if len(data) > 8192:
            data = data[:8192]
        self.sent += data
        return len(data)


def mockselect(r, w, x, timeout=0):
    """Simple mock for select()
    """
    readable = filter(lambda s: s.ready_for_read, r)
    return readable, w[:], []


def mocksslwrap(sock, keyfile=None, certfile=None,
                server_side=False, cert_reqs=http.socketutil.CERT_NONE,
                ssl_version=http.socketutil.PROTOCOL_SSLv23, ca_certs=None,
                do_handshake_on_connect=True,
                suppress_ragged_eofs=True):
    return sock


def mockgetaddrinfo(host, port, unused, streamtype):
    assert unused == 0
    assert streamtype == socket.SOCK_STREAM
    if host.count('.') != 3:
        host = '127.0.0.42'
    return [(socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP, '',
             (host, port))]


class HttpTestBase(object):
    def setUp(self):
        self.orig_socket = socket.socket
        socket.socket = MockSocket

        self.orig_getaddrinfo = socket.getaddrinfo
        socket.getaddrinfo = mockgetaddrinfo

        self.orig_select = http.select.select
        http.select.select = mockselect

        self.orig_sslwrap = http.socketutil.wrap_socket
        http.socketutil.wrap_socket = mocksslwrap

    def tearDown(self):
        socket.socket = self.orig_socket
        http.select.select = self.orig_select
        http.socketutil.wrap_socket = self.orig_sslwrap
        socket.getaddrinfo = self.orig_getaddrinfo

    def assertStringEqual(self, l, r):
        try:
            self.assertEqual(l, r, ('failed string equality check, '
                                    'see stdout for details'))
        except:
            add_nl = lambda li: map(lambda x: x + '\n', li)
            print 'failed expectation:'
            print ''.join(difflib.unified_diff(
                add_nl(l.splitlines()), add_nl(r.splitlines()),
                fromfile='expected', tofile='got'))
            raise
# no-check-code
