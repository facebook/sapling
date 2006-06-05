# hgweb/server.py - The standalone hg web server.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.demandload import demandload
import os, sys, errno
demandload(globals(), "urllib BaseHTTPServer socket SocketServer")
demandload(globals(), "mercurial:ui,hg,util,templater")
demandload(globals(), "hgweb_mod:hgweb hgwebdir_mod:hgwebdir request:hgrequest")
from mercurial.i18n import gettext as _

def _splitURI(uri):
    """ Return path and query splited from uri

    Just like CGI environment, the path is unquoted, the query is
    not.
    """
    if '?' in uri:
        path, query = uri.split('?', 1)
    else:
        path, query = uri, ''
    return urllib.unquote(path), query

class _hgwebhandler(object, BaseHTTPServer.BaseHTTPRequestHandler):
    def __init__(self, *args, **kargs):
        BaseHTTPServer.BaseHTTPRequestHandler.__init__(self, *args, **kargs)

    def log_error(self, format, *args):
        errorlog = self.server.errorlog
        errorlog.write("%s - - [%s] %s\n" % (self.address_string(),
                                             self.log_date_time_string(),
                                             format % args))

    def log_message(self, format, *args):
        accesslog = self.server.accesslog
        accesslog.write("%s - - [%s] %s\n" % (self.address_string(),
                                              self.log_date_time_string(),
                                              format % args))

    def do_POST(self):
        try:
            self.do_hgweb()
        except socket.error, inst:
            if inst[0] != errno.EPIPE:
                raise

    def do_GET(self):
        self.do_POST()

    def do_hgweb(self):
        path_info, query = _splitURI(self.path)

        env = {}
        env['GATEWAY_INTERFACE'] = 'CGI/1.1'
        env['REQUEST_METHOD'] = self.command
        env['SERVER_NAME'] = self.server.server_name
        env['SERVER_PORT'] = str(self.server.server_port)
        env['REQUEST_URI'] = "/"
        env['PATH_INFO'] = path_info
        if query:
            env['QUERY_STRING'] = query
        host = self.address_string()
        if host != self.client_address[0]:
            env['REMOTE_HOST'] = host
            env['REMOTE_ADDR'] = self.client_address[0]

        if self.headers.typeheader is None:
            env['CONTENT_TYPE'] = self.headers.type
        else:
            env['CONTENT_TYPE'] = self.headers.typeheader
        length = self.headers.getheader('content-length')
        if length:
            env['CONTENT_LENGTH'] = length
        accept = []
        for line in self.headers.getallmatchingheaders('accept'):
            if line[:1] in "\t\n\r ":
                accept.append(line.strip())
            else:
                accept = accept + line[7:].split(',')
        env['HTTP_ACCEPT'] = ','.join(accept)

        req = hgrequest(self.rfile, self.wfile, env)
        self.send_response(200, "Script output follows")
        self.server.make_and_run_handler(req)

def create_server(ui, repo):
    use_threads = True

    def openlog(opt, default):
        if opt and opt != '-':
            return open(opt, 'w')
        return default

    address = ui.config("web", "address", "")
    port = int(ui.config("web", "port", 8000))
    use_ipv6 = ui.configbool("web", "ipv6")
    webdir_conf = ui.config("web", "webdir_conf")
    accesslog = openlog(ui.config("web", "accesslog", "-"), sys.stdout)
    errorlog = openlog(ui.config("web", "errorlog", "-"), sys.stderr)

    if use_threads:
        try:
            from threading import activeCount
        except ImportError:
            use_threads = False

    if use_threads:
        _mixin = SocketServer.ThreadingMixIn
    else:
        if hasattr(os, "fork"):
            _mixin = SocketServer.ForkingMixIn
        else:
            class _mixin: pass

    class MercurialHTTPServer(object, _mixin, BaseHTTPServer.HTTPServer):
        def __init__(self, *args, **kargs):
            BaseHTTPServer.HTTPServer.__init__(self, *args, **kargs)
            self.accesslog = accesslog
            self.errorlog = errorlog
            self.repo = repo
            self.webdir_conf = webdir_conf
            self.webdirmaker = hgwebdir
            self.repoviewmaker = hgweb

        def make_and_run_handler(self, req):
            if self.webdir_conf:
                hgwebobj = self.webdirmaker(self.webdir_conf)
            elif self.repo is not None:
                hgwebobj = self.repoviewmaker(repo.__class__(repo.ui,
                                                             repo.origroot))
            else:
                raise hg.RepoError(_('no repo found'))
            hgwebobj.run(req)

    class IPv6HTTPServer(MercurialHTTPServer):
        address_family = getattr(socket, 'AF_INET6', None)

        def __init__(self, *args, **kwargs):
            if self.address_family is None:
                raise hg.RepoError(_('IPv6 not available on this system'))
            super(IPv6HTTPServer, self).__init__(*args, **kargs)

    if use_ipv6:
        return IPv6HTTPServer((address, port), _hgwebhandler)
    else:
        return MercurialHTTPServer((address, port), _hgwebhandler)
