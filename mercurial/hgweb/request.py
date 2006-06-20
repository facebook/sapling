# hgweb/request.py - An http request from either CGI or the standalone server.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.demandload import demandload
demandload(globals(), "socket sys cgi os errno")
from mercurial.i18n import gettext as _

class hgrequest(object):
    def __init__(self, inp=None, out=None, env=None):
        self.inp = inp or sys.stdin
        self.out = out or sys.stdout
        self.env = env or os.environ
        self.form = cgi.parse(self.inp, self.env, keep_blank_values=1)
        self.will_close = True

    def read(self, count=-1):
        return self.inp.read(count)

    def write(self, *things):
        for thing in things:
            if hasattr(thing, "__iter__"):
                for part in thing:
                    self.write(part)
            else:
                try:
                    self.out.write(str(thing))
                except socket.error, inst:
                    if inst[0] != errno.ECONNRESET:
                        raise

    def done(self):
        if self.will_close:
            self.inp.close()
            self.out.close()
        else:
            self.out.flush()

    def header(self, headers=[('Content-type','text/html')]):
        for header in headers:
            self.out.write("%s: %s\r\n" % header)
        self.out.write("\r\n")

    def httphdr(self, type, filename=None, length=0, headers={}):
        headers = headers.items()
        headers.append(('Content-type', type))
        if filename:
            headers.append(('Content-disposition', 'attachment; filename=%s' %
                            filename))
        # we do not yet support http 1.1 chunked transfer, so we have
        # to force connection to close if content-length not known
        if length:
            headers.append(('Content-length', str(length)))
            self.will_close = False
        else:
            headers.append(('Connection', 'close'))
            self.will_close = True
        self.header(headers)
