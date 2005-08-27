# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
import util
from node import *
from revlog import *
from repo import *
from demandload import *
demandload(globals(), "localrepo httprepo sshrepo")

# used to avoid circular references so destructors work
def opener(base):
    p = base
    def o(path, mode="r"):
        if p.startswith("http://"):
            f = os.path.join(p, urllib.quote(path))
            return httprangereader.httprangereader(f)

        f = os.path.join(p, path)

        mode += "b" # for that other OS

        if mode[0] != "r":
            try:
                s = os.stat(f)
            except OSError:
                d = os.path.dirname(f)
                if not os.path.isdir(d):
                    os.makedirs(d)
            else:
                if s.st_nlink > 1:
                    file(f + ".tmp", "wb").write(file(f, "rb").read())
                    util.rename(f+".tmp", f)

        return file(f, mode)

    return o

def repository(ui, path=None, create=0):
    if path:
        if path.startswith("http://"):
            return httprepo.httprepository(ui, path)
        if path.startswith("https://"):
            return httprepo.httpsrepository(ui, path)
        if path.startswith("hg://"):
            return httprepo.httprepository(
                ui, path.replace("hg://", "http://"))
        if path.startswith("old-http://"):
            return localrepo.localrepository(
                ui, opener, path.replace("old-http://", "http://"))
        if path.startswith("ssh://"):
            return sshrepo.sshrepository(ui, path)

    return localrepo.localrepository(ui, opener, path, create)
