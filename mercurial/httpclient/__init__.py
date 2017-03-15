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
"""Improved HTTP/1.1 client library

This library contains an HTTPConnection which is similar to the one in
httplib, but has several additional features:

  * supports keepalives natively
  * uses select() to block for incoming data
  * notices when the server responds early to a request
  * implements ssl inline instead of in a different class
"""
from __future__ import absolute_import

# Many functions in this file have too many arguments.
# pylint: disable=R0913
import email
import email.message
import errno
import inspect
import logging
import select
import socket
import ssl
import sys

try:
    import cStringIO as io
    io.StringIO
except ImportError:
    import io

try:
    import httplib
    httplib.HTTPException
except ImportError:
    import http.client as httplib

from . import (
    _readers,
)

logger = logging.getLogger(__name__)

__all__ = ['HTTPConnection', 'HTTPResponse']

HTTP_VER_1_0 = b'HTTP/1.0'
HTTP_VER_1_1 = b'HTTP/1.1'

OUTGOING_BUFFER_SIZE = 1 << 15
INCOMING_BUFFER_SIZE = 1 << 20

HDR_ACCEPT_ENCODING = 'accept-encoding'
HDR_CONNECTION_CTRL = 'connection'
HDR_CONTENT_LENGTH = 'content-length'
HDR_XFER_ENCODING = 'transfer-encoding'

XFER_ENCODING_CHUNKED = 'chunked'

CONNECTION_CLOSE = 'close'

EOL = b'\r\n'
_END_HEADERS = EOL * 2

# Based on some searching around, 1 second seems like a reasonable
# default here.
TIMEOUT_ASSUME_CONTINUE = 1
TIMEOUT_DEFAULT = None

if sys.version_info > (3, 0):
    _unicode = str
else:
    _unicode = unicode

def _ensurebytes(data):
    if not isinstance(data, (_unicode, bytes)):
        data = str(data)
    if not isinstance(data, bytes):
        try:
            return data.encode('latin-1')
        except UnicodeEncodeError as err:
            raise UnicodeEncodeError(
                err.encoding,
                err.object,
                err.start,
                err.end,
                '%r is not valid Latin-1 Use .encode("utf-8") '
                'if sending as utf-8 is desired.' % (
                    data[err.start:err.end],))
    return data

class _CompatMessage(email.message.Message):
    """Workaround for rfc822.Message and email.message.Message API diffs."""

    @classmethod
    def from_string(cls, s):
        if sys.version_info > (3, 0):
            # Python 3 can't decode headers from bytes, so we have to
            # trust RFC 2616 and decode the headers as iso-8859-1
            # bytes.
            s = s.decode('iso-8859-1')
        headers = email.message_from_string(s, _class=_CompatMessage)
        # Fix multi-line headers to match httplib's behavior from
        # Python 2.x, since email.message.Message handles them in
        # slightly different ways.
        if sys.version_info < (3, 0):
            new = []
            for h, v in headers._headers:
                if '\r\n' in v:
                    v = '\n'.join([' ' + x.lstrip() for x in v.split('\r\n')])[1:]
                new.append((h, v))
            headers._headers = new
        return headers

    def getheaders(self, key):
        return self.get_all(key)

    def getheader(self, key, default=None):
        return self.get(key, failobj=default)


class HTTPResponse(object):
    """Response from an HTTP server.

    The response will continue to load as available. If you need the
    complete response before continuing, check the .complete() method.
    """
    def __init__(self, sock, timeout, method):
        self.sock = sock
        self.method = method
        self.raw_response = b''
        self._headers_len = 0
        self.headers = None
        self.will_close = False
        self.status_line = b''
        self.status = None
        self.continued = False
        self.http_version = None
        self.reason = None
        self._reader = None

        self._read_location = 0
        self._eol = EOL

        self._timeout = timeout

    @property
    def _end_headers(self):
        return self._eol * 2

    def complete(self):
        """Returns true if this response is completely loaded.

        Note that if this is a connection where complete means the
        socket is closed, this will nearly always return False, even
        in cases where all the data has actually been loaded.
        """
        if self._reader:
            return self._reader.done()

    def _close(self):
        if self._reader is not None:
            # We're a friend of the reader class here.
            # pylint: disable=W0212
            self._reader._close()

    def getheader(self, header, default=None):
        return self.headers.getheader(header, default=default)

    def getheaders(self):
        if sys.version_info < (3, 0):
            return [(k.lower(), v) for k, v in self.headers.items()]
        # Starting in Python 3, headers aren't lowercased before being
        # returned here.
        return self.headers.items()

    def readline(self):
        """Read a single line from the response body.

        This may block until either a line ending is found or the
        response is complete.
        """
        blocks = []
        while True:
            self._reader.readto(b'\n', blocks)

            if blocks and blocks[-1][-1:] == b'\n' or self.complete():
                break

            self._select()

        return b''.join(blocks)

    def read(self, length=None):
        """Read data from the response body."""
        # if length is None, unbounded read
        while (not self.complete()  # never select on a finished read
               and (not length  # unbounded, so we wait for complete()
                    or length > self._reader.available_data)):
            self._select()
        if not length:
            length = self._reader.available_data
        r = self._reader.read(length)
        if self.complete() and self.will_close:
            self.sock.close()
        return r

    def _select(self):
        r, unused_write, unused_err = select.select(
            [self.sock], [], [], self._timeout)
        if not r:
            # socket was not readable. If the response is not
            # complete, raise a timeout.
            if not self.complete():
                logger.info('timed out with timeout of %s', self._timeout)
                raise HTTPTimeoutException('timeout reading data')
        try:
            data = self.sock.recv(INCOMING_BUFFER_SIZE)
        except ssl.SSLError as e:
            if e.args[0] != ssl.SSL_ERROR_WANT_READ:
                raise
            logger.debug('SSL_ERROR_WANT_READ in _select, should retry later')
            return True
        logger.debug('response read %d data during _select', len(data))
        # If the socket was readable and no data was read, that means
        # the socket was closed. Inform the reader (if any) so it can
        # raise an exception if this is an invalid situation.
        if not data:
            if self._reader:
                # We're a friend of the reader class here.
                # pylint: disable=W0212
                self._reader._close()
            return False
        else:
            self._load_response(data)
            return True

    # This method gets replaced by _load later, which confuses pylint.
    def _load_response(self, data): # pylint: disable=E0202
        # Being here implies we're not at the end of the headers yet,
        # since at the end of this method if headers were completely
        # loaded we replace this method with the load() method of the
        # reader we created.
        self.raw_response += data
        # This is a bogus server with bad line endings
        if self._eol not in self.raw_response:
            for bad_eol in (b'\n', b'\r'):
                if (bad_eol in self.raw_response
                    # verify that bad_eol is not the end of the incoming data
                    # as this could be a response line that just got
                    # split between \r and \n.
                    and (self.raw_response.index(bad_eol) <
                         (len(self.raw_response) - 1))):
                    logger.info('bogus line endings detected, '
                                'using %r for EOL', bad_eol)
                    self._eol = bad_eol
                    break
        # exit early if not at end of headers
        if self._end_headers not in self.raw_response or self.headers:
            return

        # handle 100-continue response
        hdrs, body = self.raw_response.split(self._end_headers, 1)
        unused_http_ver, status = hdrs.split(b' ', 1)
        if status.startswith(b'100'):
            self.raw_response = body
            self.continued = True
            logger.debug('continue seen, setting body to %r', body)
            return

        # arriving here means we should parse response headers
        # as all headers have arrived completely
        hdrs, body = self.raw_response.split(self._end_headers, 1)
        del self.raw_response
        if self._eol in hdrs:
            self.status_line, hdrs = hdrs.split(self._eol, 1)
        else:
            self.status_line = hdrs
            hdrs = b''
        # TODO HTTP < 1.0 support
        (self.http_version, self.status,
         self.reason) = self.status_line.split(b' ', 2)
        self.status = int(self.status)
        if self._eol != EOL:
            hdrs = hdrs.replace(self._eol, b'\r\n')
        headers = _CompatMessage.from_string(hdrs)
        content_len = None
        if HDR_CONTENT_LENGTH in headers:
            content_len = int(headers[HDR_CONTENT_LENGTH])
        if self.http_version == HTTP_VER_1_0:
            self.will_close = True
        elif HDR_CONNECTION_CTRL in headers:
            self.will_close = (
                headers[HDR_CONNECTION_CTRL].lower() == CONNECTION_CLOSE)
        if (HDR_XFER_ENCODING in headers
            and headers[HDR_XFER_ENCODING].lower() == XFER_ENCODING_CHUNKED):
            self._reader = _readers.ChunkedReader(self._eol)
            logger.debug('using a chunked reader')
        else:
            # HEAD responses are forbidden from returning a body, and
            # it's implausible for a CONNECT response to use
            # close-is-end logic for an OK response.
            if (self.method == b'HEAD' or
                (self.method == b'CONNECT' and content_len is None)):
                content_len = 0
            if content_len is not None:
                logger.debug('using a content-length reader with length %d',
                             content_len)
                self._reader = _readers.ContentLengthReader(content_len)
            else:
                # Response body had no length specified and is not
                # chunked, so the end of the body will only be
                # identifiable by the termination of the socket by the
                # server. My interpretation of the spec means that we
                # are correct in hitting this case if
                # transfer-encoding, content-length, and
                # connection-control were left unspecified.
                self._reader = _readers.CloseIsEndReader()
                logger.debug('using a close-is-end reader')
                self.will_close = True

        if body:
            # We're a friend of the reader class here.
            # pylint: disable=W0212
            self._reader._load(body)
        logger.debug('headers complete')
        self.headers = headers
        # We're a friend of the reader class here.
        # pylint: disable=W0212
        self._load_response = self._reader._load

def _foldheaders(headers):
    """Given some headers, rework them so we can safely overwrite values.

    >>> _foldheaders({'Accept-Encoding': 'wat'})
    {'accept-encoding': ('Accept-Encoding', 'wat')}
    """
    return dict((k.lower(), (k, v)) for k, v in headers.items())

try:
    inspect.signature
    def _handlesarg(func, arg):
        """ Try to determine if func accepts arg

        If it takes arg, return True
        If it happens to take **args, then it could do anything:
            * It could throw a different TypeError, just for fun
            * It could throw an ArgumentError or anything else
            * It could choose not to throw an Exception at all
        ... return 'unknown'

        Otherwise, return False
        """
        params = inspect.signature(func).parameters
        if arg in params:
            return True
        for p in params:
            if params[p].kind == inspect._ParameterKind.VAR_KEYWORD:
                return 'unknown'
        return False
except AttributeError:
    def _handlesarg(func, arg):
        """ Try to determine if func accepts arg

        If it takes arg, return True
        If it happens to take **args, then it could do anything:
            * It could throw a different TypeError, just for fun
            * It could throw an ArgumentError or anything else
            * It could choose not to throw an Exception at all
        ... return 'unknown'

        Otherwise, return False
        """
        spec = inspect.getargspec(func)
        if arg in spec.args:
            return True
        if spec.keywords:
            return 'unknown'
        return False

class HTTPConnection(object):
    """Connection to a single http server.

    Supports 100-continue and keepalives natively. Uses select() for
    non-blocking socket operations.
    """
    http_version = HTTP_VER_1_1
    response_class = HTTPResponse

    def __init__(self, host, port=None, use_ssl=None, ssl_validator=None,
                 timeout=TIMEOUT_DEFAULT,
                 continue_timeout=TIMEOUT_ASSUME_CONTINUE,
                 proxy_hostport=None, proxy_headers=None,
                 ssl_wrap_socket=None, **ssl_opts):
        """Create a new HTTPConnection.

        Args:
          host: The host to which we'll connect.
          port: Optional. The port over which we'll connect. Default 80 for
                non-ssl, 443 for ssl.
          use_ssl: Optional. Whether to use ssl. Defaults to False if port is
                   not 443, true if port is 443.
          ssl_validator: a function(socket) to validate the ssl cert
          timeout: Optional. Connection timeout, default is TIMEOUT_DEFAULT.
          continue_timeout: Optional. Timeout for waiting on an expected
                   "100 Continue" response. Default is TIMEOUT_ASSUME_CONTINUE.
          proxy_hostport: Optional. Tuple of (host, port) to use as an http
                       proxy for the connection. Default is to not use a proxy.
          proxy_headers: Optional dict of header keys and values to send to
                         a proxy when using CONNECT. For compatibility with
                         httplib, the Proxy-Authorization header may be
                         specified in headers for request(), which will clobber
                         any such header specified here if specified. Providing
                         this option and not proxy_hostport will raise an
                         ValueError.
          ssl_wrap_socket: Optional function to use for wrapping
            sockets. If unspecified, the one from the ssl module will
            be used if available, or something that's compatible with
            it if on a Python older than 2.6.

        Any extra keyword arguments to this function will be provided
        to the ssl_wrap_socket method. If no ssl
        """
        host = _ensurebytes(host)
        if port is None and host.count(b':') == 1 or b']:' in host:
            host, port = host.rsplit(b':', 1)
            port = int(port)
            if b'[' in host:
                host = host[1:-1]
        if ssl_wrap_socket is not None:
            _wrap_socket = ssl_wrap_socket
        else:
            _wrap_socket = ssl.wrap_socket
        call_wrap_socket = None
        handlesubar = _handlesarg(_wrap_socket, 'server_hostname')
        if handlesubar is True:
            # supports server_hostname
            call_wrap_socket = _wrap_socket
        handlesnobar = _handlesarg(_wrap_socket, 'serverhostname')
        if handlesnobar is True and handlesubar is not True:
            # supports serverhostname
            def call_wrap_socket(sock, server_hostname=None, **ssl_opts):
                return _wrap_socket(sock, serverhostname=server_hostname,
                                    **ssl_opts)
        if handlesubar is False and handlesnobar is False:
            # does not support either
            def call_wrap_socket(sock, server_hostname=None, **ssl_opts):
                return _wrap_socket(sock, **ssl_opts)
        if call_wrap_socket is None:
            # we assume it takes **args
            def call_wrap_socket(sock, **ssl_opts):
                if 'server_hostname' in ssl_opts:
                    ssl_opts['serverhostname'] = ssl_opts['server_hostname']
                return _wrap_socket(sock, **ssl_opts)
        self._ssl_wrap_socket = call_wrap_socket
        if use_ssl is None and port is None:
            use_ssl = False
            port = 80
        elif use_ssl is None:
            use_ssl = (port == 443)
        elif port is None:
            port = (use_ssl and 443 or 80)
        self.port = port
        self.ssl = use_ssl
        self.ssl_opts = ssl_opts
        self._ssl_validator = ssl_validator
        self.host = host
        self.sock = None
        self._current_response = None
        self._current_response_taken = False
        if proxy_hostport is None:
            self._proxy_host = self._proxy_port = None
            if proxy_headers:
                raise ValueError(
                    'proxy_headers may not be specified unless '
                    'proxy_hostport is also specified.')
            else:
                self._proxy_headers = {}
        else:
            self._proxy_host, self._proxy_port = proxy_hostport
            self._proxy_headers = _foldheaders(proxy_headers or {})

        self.timeout = timeout
        self.continue_timeout = continue_timeout

    def _connect(self, proxy_headers):
        """Connect to the host and port specified in __init__."""
        if self.sock:
            return
        if self._proxy_host is not None:
            logger.info('Connecting to http proxy %s:%s',
                        self._proxy_host, self._proxy_port)
            sock = socket.create_connection((self._proxy_host,
                                             self._proxy_port))
            if self.ssl:
                data = self._buildheaders(b'CONNECT', b'%s:%d' % (self.host,
                                                                  self.port),
                                          proxy_headers, HTTP_VER_1_0)
                sock.send(data)
                sock.setblocking(0)
                r = self.response_class(sock, self.timeout, b'CONNECT')
                timeout_exc = HTTPTimeoutException(
                    'Timed out waiting for CONNECT response from proxy')
                while not r.complete():
                    try:
                        # We're a friend of the response class, so let
                        # us use the private attribute.
                        # pylint: disable=W0212
                        if not r._select():
                            if not r.complete():
                                raise timeout_exc
                    except HTTPTimeoutException:
                        # This raise/except pattern looks goofy, but
                        # _select can raise the timeout as well as the
                        # loop body. I wish it wasn't this convoluted,
                        # but I don't have a better solution
                        # immediately handy.
                        raise timeout_exc
                if r.status != 200:
                    raise HTTPProxyConnectFailedException(
                        'Proxy connection failed: %d %s' % (r.status,
                                                            r.read()))
                logger.info('CONNECT (for SSL) to %s:%s via proxy succeeded.',
                            self.host, self.port)
        else:
            sock = socket.create_connection((self.host, self.port))
        if self.ssl:
            # This is the default, but in the case of proxied SSL
            # requests the proxy logic above will have cleared
            # blocking mode, so re-enable it just to be safe.
            sock.setblocking(1)
            logger.debug('wrapping socket for ssl with options %r',
                         self.ssl_opts)
            sock = self._ssl_wrap_socket(sock, server_hostname=self.host,
                                         **self.ssl_opts)
            if self._ssl_validator:
                self._ssl_validator(sock)
        sock.setblocking(0)
        self.sock = sock

    def _buildheaders(self, method, path, headers, http_ver):
        if self.ssl and self.port == 443 or self.port == 80:
            # default port for protocol, so leave it out
            hdrhost = self.host
        else:
            # include nonstandard port in header
            if b':' in self.host:  # must be IPv6
                hdrhost = b'[%s]:%d' % (self.host, self.port)
            else:
                hdrhost = b'%s:%d' % (self.host, self.port)
        if self._proxy_host and not self.ssl:
            # When talking to a regular http proxy we must send the
            # full URI, but in all other cases we must not (although
            # technically RFC 2616 says servers must accept our
            # request if we screw up, experimentally few do that
            # correctly.)
            assert path[0:1] == b'/', 'path must start with a /'
            path = b'http://%s%s' % (hdrhost, path)
        outgoing = [b'%s %s %s%s' % (method, path, http_ver, EOL)]
        headers[b'host'] = (b'Host', hdrhost)
        headers[HDR_ACCEPT_ENCODING] = (HDR_ACCEPT_ENCODING, 'identity')
        for hdr, val in sorted((_ensurebytes(h), _ensurebytes(v))
                               for h, v in headers.values()):
            outgoing.append(b'%s: %s%s' % (hdr, val, EOL))
        outgoing.append(EOL)
        return b''.join(outgoing)

    def close(self):
        """Close the connection to the server.

        This is a no-op if the connection is already closed. The
        connection may automatically close if requested by the server
        or required by the nature of a response.
        """
        if self.sock is None:
            return
        self.sock.close()
        self.sock = None
        logger.info('closed connection to %s on %s', self.host, self.port)

    def busy(self):
        """Returns True if this connection object is currently in use.

        If a response is still pending, this will return True, even if
        the request has finished sending. In the future,
        HTTPConnection may transparently juggle multiple connections
        to the server, in which case this will be useful to detect if
        any of those connections is ready for use.
        """
        cr = self._current_response
        if cr is not None:
            if self._current_response_taken:
                if cr.will_close:
                    self.sock = None
                    self._current_response = None
                    return False
                elif cr.complete():
                    self._current_response = None
                    return False
            return True
        return False

    def _reconnect(self, where, pheaders):
        logger.info('reconnecting during %s', where)
        self.close()
        self._connect(pheaders)

    def request(self, method, path, body=None, headers=None,
                expect_continue=False):
        """Send a request to the server.

        For increased flexibility, this does not return the response
        object. Future versions of HTTPConnection that juggle multiple
        sockets will be able to send (for example) 5 requests all at
        once, and then let the requests arrive as data is
        available. Use the `getresponse()` method to retrieve the
        response.
        """
        if headers is None:
            headers = {}
        method = _ensurebytes(method)
        path = _ensurebytes(path)
        if self.busy():
            raise httplib.CannotSendRequest(
                'Can not send another request before '
                'current response is read!')
        self._current_response_taken = False

        logger.info('sending %s request for %s to %s on port %s',
                    method, path, self.host, self.port)

        hdrs = _foldheaders(headers)
        # Figure out headers that have to be computed from the request
        # body.
        chunked = False
        if body and HDR_CONTENT_LENGTH not in hdrs:
            if getattr(body, '__len__', False):
                hdrs[HDR_CONTENT_LENGTH] = (HDR_CONTENT_LENGTH,
                                            b'%d' % len(body))
            elif getattr(body, 'read', False):
                hdrs[HDR_XFER_ENCODING] = (HDR_XFER_ENCODING,
                                           XFER_ENCODING_CHUNKED)
                chunked = True
            else:
                raise BadRequestData('body has no __len__() nor read()')
        # Figure out expect-continue header
        if hdrs.get('expect', ('', ''))[1].lower() == b'100-continue':
            expect_continue = True
        elif expect_continue:
            hdrs['expect'] = (b'Expect', b'100-Continue')
        # httplib compatibility: if the user specified a
        # proxy-authorization header, that's actually intended for a
        # proxy CONNECT action, not the real request, but only if
        # we're going to use a proxy.
        pheaders = dict(self._proxy_headers)
        if self._proxy_host and self.ssl:
            pa = hdrs.pop('proxy-authorization', None)
            if pa is not None:
                pheaders['proxy-authorization'] = pa
        # Build header data
        outgoing_headers = self._buildheaders(
            method, path, hdrs, self.http_version)

        # If we're reusing the underlying socket, there are some
        # conditions where we'll want to retry, so make a note of the
        # state of self.sock
        fresh_socket = self.sock is None
        self._connect(pheaders)
        response = None
        first = True

        while ((outgoing_headers or body)
               and not (response and response.complete())):
            select_timeout = self.timeout
            out = outgoing_headers or body
            blocking_on_continue = False
            if expect_continue and not outgoing_headers and not (
                response and (response.headers or response.continued)):
                logger.info(
                    'waiting up to %s seconds for'
                    ' continue response from server',
                    self.continue_timeout)
                select_timeout = self.continue_timeout
                blocking_on_continue = True
                out = False
            if out:
                w = [self.sock]
            else:
                w = []
            r, w, x = select.select([self.sock], w, [], select_timeout)
            # if we were expecting a 100 continue and it's been long
            # enough, just go ahead and assume it's ok. This is the
            # recommended behavior from the RFC.
            if r == w == x == []:
                if blocking_on_continue:
                    expect_continue = False
                    logger.info('no response to continue expectation from '
                                'server, optimistically sending request body')
                else:
                    raise HTTPTimeoutException('timeout sending data')
            was_first = first

            # incoming data
            if r:
                try:
                    try:
                        data = r[0].recv(INCOMING_BUFFER_SIZE)
                    except ssl.SSLError as e:
                        if e.args[0] != ssl.SSL_ERROR_WANT_READ:
                            raise
                        logger.debug('SSL_ERROR_WANT_READ while sending '
                                     'data, retrying...')
                        continue
                    if not data:
                        logger.info('socket appears closed in read')
                        self.sock = None
                        self._current_response = None
                        if response is not None:
                            # We're a friend of the response class, so let
                            # us use the private attribute.
                            # pylint: disable=W0212
                            response._close()
                        # This if/elif ladder is a bit subtle,
                        # comments in each branch should help.
                        if response is not None and response.complete():
                            # Server responded completely and then
                            # closed the socket. We should just shut
                            # things down and let the caller get their
                            # response.
                            logger.info('Got an early response, '
                                        'aborting remaining request.')
                            break
                        elif was_first and response is None:
                            # Most likely a keepalive that got killed
                            # on the server's end. Commonly happens
                            # after getting a really large response
                            # from the server.
                            logger.info(
                                'Connection appeared closed in read on first'
                                ' request loop iteration, will retry.')
                            self._reconnect('read', pheaders)
                            continue
                        else:
                            # We didn't just send the first data hunk,
                            # and either have a partial response or no
                            # response at all. There's really nothing
                            # meaningful we can do here.
                            raise HTTPStateError(
                                'Connection appears closed after '
                                'some request data was written, but the '
                                'response was missing or incomplete!')
                    logger.debug('read %d bytes in request()', len(data))
                    if response is None:
                        response = self.response_class(
                            r[0], self.timeout, method)
                    # We're a friend of the response class, so let us
                    # use the private attribute.
                    # pylint: disable=W0212
                    response._load_response(data)
                    # Jump to the next select() call so we load more
                    # data if the server is still sending us content.
                    continue
                except socket.error as e:
                    if e[0] != errno.EPIPE and not was_first:
                        raise

            # outgoing data
            if w and out:
                try:
                    if getattr(out, 'read', False):
                        # pylint guesses the type of out incorrectly here
                        # pylint: disable=E1103
                        data = out.read(OUTGOING_BUFFER_SIZE)
                        if not data:
                            continue
                        if len(data) < OUTGOING_BUFFER_SIZE:
                            if chunked:
                                body = b'0' + EOL + EOL
                            else:
                                body = None
                        if chunked:
                            # This encode is okay because we know
                            # hex() is building us only 0-9 and a-f
                            # digits.
                            asciilen = hex(len(data))[2:].encode('ascii')
                            out = asciilen + EOL + data + EOL
                        else:
                            out = data
                    amt = w[0].send(out)
                except socket.error as e:
                    if e[0] == ssl.SSL_ERROR_WANT_WRITE and self.ssl:
                        # This means that SSL hasn't flushed its buffer into
                        # the socket yet.
                        # TODO: find a way to block on ssl flushing its buffer
                        # similar to selecting on a raw socket.
                        continue
                    if e[0] == errno.EWOULDBLOCK or e[0] == errno.EAGAIN:
                        continue
                    elif (e[0] not in (errno.ECONNRESET, errno.EPIPE)
                          and not first):
                        raise
                    self._reconnect('write', pheaders)
                    amt = self.sock.send(out)
                logger.debug('sent %d', amt)
                first = False
                if out is body:
                    body = out[amt:]
                else:
                    outgoing_headers = out[amt:]
        # End of request-sending loop.

        # close if the server response said to or responded before eating
        # the whole request
        if response is None:
            response = self.response_class(self.sock, self.timeout, method)
            if not fresh_socket:
                if not response._select():
                    # This means the response failed to get any response
                    # data at all, and in all probability the socket was
                    # closed before the server even saw our request. Try
                    # the request again on a fresh socket.
                    logger.debug('response._select() failed during request().'
                                 ' Assuming request needs to be retried.')
                    self.sock = None
                    # Call this method explicitly to re-try the
                    # request. We don't use self.request() because
                    # some tools (notably Mercurial) expect to be able
                    # to subclass and redefine request(), and they
                    # don't have the same argspec as we do.
                    #
                    # TODO restructure sending of requests to avoid
                    # this recursion
                    return HTTPConnection.request(
                        self, method, path, body=body, headers=headers,
                        expect_continue=expect_continue)
        data_left = bool(outgoing_headers or body)
        if data_left:
            logger.info('stopped sending request early, '
                         'will close the socket to be safe.')
            response.will_close = True
        if response.will_close:
            # The socket will be closed by the response, so we disown
            # the socket
            self.sock = None
        self._current_response = response

    def getresponse(self):
        """Returns the response to the most recent request."""
        if self._current_response is None:
            raise httplib.ResponseNotReady()
        r = self._current_response
        while r.headers is None:
            # We're a friend of the response class, so let us use the
            # private attribute.
            # pylint: disable=W0212
            if not r._select() and not r.complete():
                raise _readers.HTTPRemoteClosedError()
        if r.will_close:
            self.sock = None
            self._current_response = None
        elif r.complete():
            self._current_response = None
        else:
            self._current_response_taken = True
        return r


class HTTPTimeoutException(httplib.HTTPException):
    """A timeout occurred while waiting on the server."""


class BadRequestData(httplib.HTTPException):
    """Request body object has neither __len__ nor read."""


class HTTPProxyConnectFailedException(httplib.HTTPException):
    """Connecting to the HTTP proxy failed."""


class HTTPStateError(httplib.HTTPException):
    """Invalid internal state encountered."""

# Forward this exception type from _readers since it needs to be part
# of the public API.
HTTPRemoteClosedError = _readers.HTTPRemoteClosedError
# no-check-code
