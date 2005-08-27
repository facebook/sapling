# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import util
from node import *
from repo import *
from demandload import *
demandload(globals(), "localrepo httprepo sshrepo statichttprepo")

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

    return localrepo.localrepository(ui, util.opener, path, create)
