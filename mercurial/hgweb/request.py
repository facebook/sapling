# hgweb/request.py - An http request from either CGI or the standalone server.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import socket, cgi, errno
from mercurial.i18n import gettext as _
from common import ErrorResponse, statusmessage

class wsgirequest(object):
    def __init__(self, wsgienv, start_response):
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
        self._start_response = start_response
        self.headers = []

    def __iter__(self):
        return iter([])

    def read(self, count=-1):
        return self.inp.read(count)

    def start_response(self, status):
        if self._start_response is not None:
            if not self.headers:
                raise RuntimeError("request.write called before headers sent")

            for k, v in self.headers:
                if not isinstance(v, str):
                    raise TypeError('header value must be string: %r' % v)

            if isinstance(status, ErrorResponse):
                status = statusmessage(status.code)
            elif isinstance(status, int):
                status = statusmessage(status)

            self.server_write = self._start_response(status, self.headers)
            self._start_response = None
            self.headers = []

    def respond(self, status, *things):
        if not things:
            self.start_response(status)
        for thing in things:
            if hasattr(thing, "__iter__"):
                for part in thing:
                    self.respond(status, part)
            else:
                thing = str(thing)
                self.start_response(status)
                try:
                    self.server_write(thing)
                except socket.error, inst:
                    if inst[0] != errno.ECONNRESET:
                        raise

    def write(self, *things):
        self.respond('200 Script output follows', *things)

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

def wsgiapplication(app_maker):
    '''For compatibility with old CGI scripts. A plain hgweb() or hgwebdir()
    can and should now be used as a WSGI application.'''
    application = app_maker()
    def run_wsgi(env, respond):
        return application(env, respond)
    return run_wsgi
