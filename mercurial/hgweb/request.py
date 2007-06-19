# hgweb/request.py - An http request from either CGI or the standalone server.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import socket, cgi, errno
from mercurial.i18n import gettext as _

class wsgiapplication(object):
    def __init__(self, destmaker):
        self.destmaker = destmaker

    def __call__(self, wsgienv, start_response):
        return _wsgirequest(self.destmaker(), wsgienv, start_response)

class _wsgirequest(object):
    def __init__(self, destination, wsgienv, start_response):
        version = wsgienv['wsgi.version']
        if (version < (1, 0)) or (version >= (2, 0)):
            raise RuntimeError("Unknown and unsupported WSGI version %d.%d"
                               % version)
        self.inp = wsgienv['wsgi.input']
        self.server_write = None
        self.err = wsgienv['wsgi.errors']
        self.threaded = wsgienv['wsgi.multithread']
        self.multiprocess = wsgienv['wsgi.multiprocess']
        self.run_once = wsgienv['wsgi.run_once']
        self.env = wsgienv
        self.form = cgi.parse(self.inp, self.env, keep_blank_values=1)
        self.start_response = start_response
        self.headers = []
        destination.run_wsgi(self)

    out = property(lambda self: self)

    def __iter__(self):
        return iter([])

    def read(self, count=-1):
        return self.inp.read(count)

    def write(self, *things):
        for thing in things:
            if hasattr(thing, "__iter__"):
                for part in thing:
                    self.write(part)
            else:
                thing = str(thing)
                if self.server_write is None:
                    if not self.headers:
                        raise RuntimeError("request.write called before headers sent (%s)." % thing)
                    self.server_write = self.start_response('200 Script output follows',
                                                            self.headers)
                    self.start_response = None
                    self.headers = None
                try:
                    self.server_write(thing)
                except socket.error, inst:
                    if inst[0] != errno.ECONNRESET:
                        raise

    def writelines(self, lines):
        for line in lines:
            self.write(line)

    def flush(self):
        return None

    def close(self):
        return None

    def header(self, headers=[('Content-type','text/html')]):
        self.headers.extend(headers)

    def httphdr(self, type, filename=None, length=0, headers={}):
        headers = headers.items()
        headers.append(('Content-type', type))
        if filename:
            headers.append(('Content-disposition', 'attachment; filename=%s' %
                            filename))
        if length:
            headers.append(('Content-length', str(length)))
        self.header(headers)
