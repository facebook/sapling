# hgweb/server.py - The standalone hg web server.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import socket
import sys
import traceback

from ..i18n import _

from .. import (
    encoding,
    error,
    pycompat,
    util,
)

httpservermod = util.httpserver
socketserver = util.socketserver
urlerr = util.urlerr
urlreq = util.urlreq

from . import (
    common,
)

def _splitURI(uri):
    """Return path and query that has been split from uri

    Just like CGI environment, the path is unquoted, the query is
    not.
    """
    if r'?' in uri:
        path, query = uri.split(r'?', 1)
    else:
        path, query = uri, r''
    return urlreq.unquote(path), query

class _error_logger(object):
    def __init__(self, handler):
        self.handler = handler
    def flush(self):
        pass
    def write(self, str):
        self.writelines(str.split('\n'))
    def writelines(self, seq):
        for msg in seq:
            self.handler.log_error("HG error:  %s", msg)

class _httprequesthandler(httpservermod.basehttprequesthandler):

    url_scheme = 'http'

    @staticmethod
    def preparehttpserver(httpserver, ui):
        """Prepare .socket of new HTTPServer instance"""

    def __init__(self, *args, **kargs):
        self.protocol_version = r'HTTP/1.1'
        httpservermod.basehttprequesthandler.__init__(self, *args, **kargs)

    def _log_any(self, fp, format, *args):
        fp.write(pycompat.sysbytes(
            r"%s - - [%s] %s" % (self.client_address[0],
                                 self.log_date_time_string(),
                                 format % args)) + '\n')
        fp.flush()

    def log_error(self, format, *args):
        self._log_any(self.server.errorlog, format, *args)

    def log_message(self, format, *args):
        self._log_any(self.server.accesslog, format, *args)

    def log_request(self, code=r'-', size=r'-'):
        xheaders = []
        if util.safehasattr(self, 'headers'):
            xheaders = [h for h in self.headers.items()
                        if h[0].startswith(r'x-')]
        self.log_message(r'"%s" %s %s%s',
                         self.requestline, str(code), str(size),
                         r''.join([r' %s:%s' % h for h in sorted(xheaders)]))

    def do_write(self):
        try:
            self.do_hgweb()
        except socket.error as inst:
            if inst[0] != errno.EPIPE:
                raise

    def do_POST(self):
        try:
            self.do_write()
        except Exception:
            self._start_response("500 Internal Server Error", [])
            self._write("Internal Server Error")
            self._done()
            tb = r"".join(traceback.format_exception(*sys.exc_info()))
            # We need a native-string newline to poke in the log
            # message, because we won't get a newline when using an
            # r-string. This is the easy way out.
            newline = chr(10)
            self.log_error(r"Exception happened during processing "
                           r"request '%s':%s%s", self.path, newline, tb)

    def do_GET(self):
        self.do_POST()

    def do_hgweb(self):
        self.sent_headers = False
        path, query = _splitURI(self.path)

        env = {}
        env[r'GATEWAY_INTERFACE'] = r'CGI/1.1'
        env[r'REQUEST_METHOD'] = self.command
        env[r'SERVER_NAME'] = self.server.server_name
        env[r'SERVER_PORT'] = str(self.server.server_port)
        env[r'REQUEST_URI'] = self.path
        env[r'SCRIPT_NAME'] = self.server.prefix
        env[r'PATH_INFO'] = path[len(self.server.prefix):]
        env[r'REMOTE_HOST'] = self.client_address[0]
        env[r'REMOTE_ADDR'] = self.client_address[0]
        if query:
            env[r'QUERY_STRING'] = query

        if pycompat.ispy3:
            if self.headers.get_content_type() is None:
                env[r'CONTENT_TYPE'] = self.headers.get_default_type()
            else:
                env[r'CONTENT_TYPE'] = self.headers.get_content_type()
            length = self.headers.get('content-length')
        else:
            if self.headers.typeheader is None:
                env[r'CONTENT_TYPE'] = self.headers.type
            else:
                env[r'CONTENT_TYPE'] = self.headers.typeheader
            length = self.headers.getheader('content-length')
        if length:
            env[r'CONTENT_LENGTH'] = length
        for header in [h for h in self.headers.keys()
                       if h not in ('content-type', 'content-length')]:
            hkey = r'HTTP_' + header.replace(r'-', r'_').upper()
            hval = self.headers.get(header)
            hval = hval.replace(r'\n', r'').strip()
            if hval:
                env[hkey] = hval
        env[r'SERVER_PROTOCOL'] = self.request_version
        env[r'wsgi.version'] = (1, 0)
        env[r'wsgi.url_scheme'] = self.url_scheme
        if env.get(r'HTTP_EXPECT', '').lower() == '100-continue':
            self.rfile = common.continuereader(self.rfile, self.wfile.write)

        env[r'wsgi.input'] = self.rfile
        env[r'wsgi.errors'] = _error_logger(self)
        env[r'wsgi.multithread'] = isinstance(self.server,
                                             socketserver.ThreadingMixIn)
        env[r'wsgi.multiprocess'] = isinstance(self.server,
                                              socketserver.ForkingMixIn)
        env[r'wsgi.run_once'] = 0

        self.saved_status = None
        self.saved_headers = []
        self.length = None
        self._chunked = None
        for chunk in self.server.application(env, self._start_response):
            self._write(chunk)
        if not self.sent_headers:
            self.send_headers()
        self._done()

    def send_headers(self):
        if not self.saved_status:
            raise AssertionError("Sending headers before "
                                 "start_response() called")
        saved_status = self.saved_status.split(None, 1)
        saved_status[0] = int(saved_status[0])
        self.send_response(*saved_status)
        self.length = None
        self._chunked = False
        for h in self.saved_headers:
            self.send_header(*h)
            if h[0].lower() == 'content-length':
                self.length = int(h[1])
        if (self.length is None and
            saved_status[0] != common.HTTP_NOT_MODIFIED):
            self._chunked = (not self.close_connection and
                             self.request_version == "HTTP/1.1")
            if self._chunked:
                self.send_header(r'Transfer-Encoding', r'chunked')
            else:
                self.send_header(r'Connection', r'close')
        self.end_headers()
        self.sent_headers = True

    def _start_response(self, http_status, headers, exc_info=None):
        code, msg = http_status.split(None, 1)
        code = int(code)
        self.saved_status = http_status
        bad_headers = ('connection', 'transfer-encoding')
        self.saved_headers = [h for h in headers
                              if h[0].lower() not in bad_headers]
        return self._write

    def _write(self, data):
        if not self.saved_status:
            raise AssertionError("data written before start_response() called")
        elif not self.sent_headers:
            self.send_headers()
        if self.length is not None:
            if len(data) > self.length:
                raise AssertionError("Content-length header sent, but more "
                                     "bytes than specified are being written.")
            self.length = self.length - len(data)
        elif self._chunked and data:
            data = '%x\r\n%s\r\n' % (len(data), data)
        self.wfile.write(data)
        self.wfile.flush()

    def _done(self):
        if self._chunked:
            self.wfile.write('0\r\n\r\n')
            self.wfile.flush()

class _httprequesthandlerssl(_httprequesthandler):
    """HTTPS handler based on Python's ssl module"""

    url_scheme = 'https'

    @staticmethod
    def preparehttpserver(httpserver, ui):
        try:
            from .. import sslutil
            sslutil.modernssl
        except ImportError:
            raise error.Abort(_("SSL support is unavailable"))

        certfile = ui.config('web', 'certificate')

        # These config options are currently only meant for testing. Use
        # at your own risk.
        cafile = ui.config('devel', 'servercafile')
        reqcert = ui.configbool('devel', 'serverrequirecert')

        httpserver.socket = sslutil.wrapserversocket(httpserver.socket,
                                                     ui,
                                                     certfile=certfile,
                                                     cafile=cafile,
                                                     requireclientcert=reqcert)

    def setup(self):
        self.connection = self.request
        self.rfile = socket._fileobject(self.request, "rb", self.rbufsize)
        self.wfile = socket._fileobject(self.request, "wb", self.wbufsize)

try:
    import threading
    threading.activeCount() # silence pyflakes and bypass demandimport
    _mixin = socketserver.ThreadingMixIn
except ImportError:
    if util.safehasattr(os, "fork"):
        _mixin = socketserver.ForkingMixIn
    else:
        class _mixin(object):
            pass

def openlog(opt, default):
    if opt and opt != '-':
        return open(opt, 'a')
    return default

class MercurialHTTPServer(_mixin, httpservermod.httpserver, object):

    # SO_REUSEADDR has broken semantics on windows
    if pycompat.iswindows:
        allow_reuse_address = 0

    def __init__(self, ui, app, addr, handler, **kwargs):
        httpservermod.httpserver.__init__(self, addr, handler, **kwargs)
        self.daemon_threads = True
        self.application = app

        handler.preparehttpserver(self, ui)

        prefix = ui.config('web', 'prefix')
        if prefix:
            prefix = '/' + prefix.strip('/')
        self.prefix = prefix

        alog = openlog(ui.config('web', 'accesslog'), ui.fout)
        elog = openlog(ui.config('web', 'errorlog'), ui.ferr)
        self.accesslog = alog
        self.errorlog = elog

        self.addr, self.port = self.socket.getsockname()[0:2]
        self.fqaddr = socket.getfqdn(addr[0])

class IPv6HTTPServer(MercurialHTTPServer):
    address_family = getattr(socket, 'AF_INET6', None)
    def __init__(self, *args, **kwargs):
        if self.address_family is None:
            raise error.RepoError(_('IPv6 is not available on this system'))
        super(IPv6HTTPServer, self).__init__(*args, **kwargs)

def create_server(ui, app):

    if ui.config('web', 'certificate'):
        handler = _httprequesthandlerssl
    else:
        handler = _httprequesthandler

    if ui.configbool('web', 'ipv6'):
        cls = IPv6HTTPServer
    else:
        cls = MercurialHTTPServer

    # ugly hack due to python issue5853 (for threaded use)
    try:
        import mimetypes
        mimetypes.init()
    except UnicodeDecodeError:
        # Python 2.x's mimetypes module attempts to decode strings
        # from Windows' ANSI APIs as ascii (fail), then re-encode them
        # as ascii (clown fail), because the default Python Unicode
        # codec is hardcoded as ascii.

        sys.argv # unwrap demand-loader so that reload() works
        reload(sys) # resurrect sys.setdefaultencoding()
        oldenc = sys.getdefaultencoding()
        sys.setdefaultencoding("latin1") # or any full 8-bit encoding
        mimetypes.init()
        sys.setdefaultencoding(oldenc)

    address = ui.config('web', 'address')
    port = util.getport(ui.config('web', 'port'))
    try:
        return cls(ui, app, (address, port), handler)
    except socket.error as inst:
        raise error.Abort(_("cannot start server at '%s:%d': %s")
                          % (address, port, encoding.strtolocal(inst.args[1])))
