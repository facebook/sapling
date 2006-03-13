# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from repo import *
from demandload import *
demandload(globals(), "localrepo bundlerepo httprepo sshrepo statichttprepo")

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
            return statichttprepo.statichttprepository(
                ui, path.replace("old-http://", "http://"))
        if path.startswith("ssh://"):
            return sshrepo.sshrepository(ui, path)
        if path.startswith("bundle://"):
            path = path[9:]
            s = path.split("+", 1)
            if  len(s) == 1:
                repopath, bundlename = "", s[0]
            else:
                repopath, bundlename = s
            return bundlerepo.bundlerepository(ui, repopath, bundlename)

    return localrepo.localrepository(ui, path, create)
