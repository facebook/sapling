# hgweb.py - web interface to a mercurial repository
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.demandload import demandload
demandload(globals(), "socket sys cgi os")
from mercurial.i18n import gettext as _

class hgrequest(object):
    def __init__(self, inp=None, out=None, env=None):
        self.inp = inp or sys.stdin
        self.out = out or sys.stdout
        self.env = env or os.environ
        self.form = cgi.parse(self.inp, self.env, keep_blank_values=1)

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

    def header(self, headers=[('Content-type','text/html')]):
        for header in headers:
            self.out.write("%s: %s\r\n" % header)
        self.out.write("\r\n")

    def httphdr(self, type, file="", size=0):

        headers = [('Content-type', type)]
        if file:
            headers.append(('Content-disposition', 'attachment; filename=%s' % file))
        if size > 0:
            headers.append(('Content-length', str(size)))
        self.header(headers)
