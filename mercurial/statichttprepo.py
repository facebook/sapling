# statichttprepo.py - simple http repository class for mercurial
#
# This provides read-only repo access to repositories exported via static http
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
demandload(globals(), "changelog filelog httprangereader")
demandload(globals(), "localrepo manifest os urllib urllib2")

class rangereader(httprangereader.httprangereader):
    def read(self, size=None):
        try:
            return httprangereader.httprangereader.read(self, size)
        except urllib2.URLError, inst:
            raise IOError(None, str(inst))

def opener(base):
    """return a function that opens files over http"""
    p = base
    def o(path, mode="r"):
        f = os.path.join(p, urllib.quote(path))
        return rangereader(f)
    return o

class statichttprepository(localrepo.localrepository):
    def __init__(self, ui, path):
        self.path = (path + "/.hg")
        self.ui = ui
        self.opener = opener(self.path)
        self.manifest = manifest.manifest(self.opener)
        self.changelog = changelog.changelog(self.opener)
        self.tagscache = None
        self.nodetagscache = None
        self.encodepats = None
        self.decodepats = None

    def dev(self):
        return -1

    def local(self):
        return False
