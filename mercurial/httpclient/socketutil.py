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
"""Abstraction to simplify socket use for Python < 2.6

This will attempt to use the ssl module and the new
socket.create_connection method, but fall back to the old
methods if those are unavailable.
"""
from __future__ import absolute_import

import logging
import socket

logger = logging.getLogger(__name__)

try:
    import ssl
    # make demandimporters load the module
    ssl.wrap_socket # pylint: disable=W0104
    have_ssl = True
except ImportError:
    import httplib
    import urllib2
    have_ssl = getattr(urllib2, 'HTTPSHandler', False)
    ssl = False


try:
    create_connection = socket.create_connection
except AttributeError:
    def create_connection(address):
        """Backport of socket.create_connection from Python 2.6."""
        host, port = address
        msg = "getaddrinfo returns an empty list"
        sock = None
        for res in socket.getaddrinfo(host, port, 0,
                                      socket.SOCK_STREAM):
            af, socktype, proto, unused_canonname, sa = res
            try:
                sock = socket.socket(af, socktype, proto)
                logger.info("connect: (%s, %s)", host, port)
                sock.connect(sa)
            except socket.error as msg:
                logger.info('connect fail: %s %s', host, port)
                if sock:
                    sock.close()
                sock = None
                continue
            break
        if not sock:
            raise socket.error(msg)
        return sock

if ssl:
    wrap_socket = ssl.wrap_socket
    CERT_NONE = ssl.CERT_NONE
    CERT_OPTIONAL = ssl.CERT_OPTIONAL
    CERT_REQUIRED = ssl.CERT_REQUIRED
else:
    class FakeSocket(httplib.FakeSocket):
        """Socket wrapper that supports SSL."""

        # Silence lint about this goofy backport class
        # pylint: disable=W0232,E1101,R0903,R0913,C0111

        # backport the behavior from Python 2.6, which is to busy wait
        # on the socket instead of anything nice. Sigh.
        # See http://bugs.python.org/issue3890 for more info.
        def recv(self, buflen=1024, flags=0):
            """ssl-aware wrapper around socket.recv
            """
            if flags != 0:
                raise ValueError(
                    "non-zero flags not allowed in calls to recv() on %s" %
                    self.__class__)
            while True:
                try:
                    return self._ssl.read(buflen)
                except socket.sslerror as x:
                    if x.args[0] == socket.SSL_ERROR_WANT_READ:
                        continue
                    else:
                        raise x

    _PROTOCOL_SSLv23 = 2

    CERT_NONE = 0
    CERT_OPTIONAL = 1
    CERT_REQUIRED = 2

    # Disable unused-argument because we're making a dumb wrapper
    # that's like an upstream method.
    #
    # pylint: disable=W0613,R0913
    def wrap_socket(sock, keyfile=None, certfile=None,
                server_side=False, cert_reqs=CERT_NONE,
                ssl_version=_PROTOCOL_SSLv23, ca_certs=None,
                do_handshake_on_connect=True,
                suppress_ragged_eofs=True):
        """Backport of ssl.wrap_socket from Python 2.6."""
        if cert_reqs != CERT_NONE and ca_certs:
            raise CertificateValidationUnsupported(
                'SSL certificate validation requires the ssl module'
                '(included in Python 2.6 and later.)')
        sslob = socket.ssl(sock)
        # borrow httplib's workaround for no ssl.wrap_socket
        sock = FakeSocket(sock, sslob)
        return sock
    # pylint: enable=W0613,R0913


class CertificateValidationUnsupported(Exception):
    """Exception raised when cert validation is requested but unavailable."""
# no-check-code
