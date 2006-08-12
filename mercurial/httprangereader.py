# httprangereader.py - just what it says
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import byterange, urllib2

class httprangereader(object):
    def __init__(self, url):
        self.url = url
        self.pos = 0
    def seek(self, pos):
        self.pos = pos
    def read(self, bytes=None):
        opener = urllib2.build_opener(byterange.HTTPRangeHandler())
        urllib2.install_opener(opener)
        req = urllib2.Request(self.url)
        end = ''
        if bytes:
            end = self.pos + bytes - 1
        req.add_header('Range', 'bytes=%d-%s' % (self.pos, end))
        f = urllib2.urlopen(req)
        data = f.read()
        if bytes:
            data = data[:bytes]
        return data
