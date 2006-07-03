# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from repo import *
from demandload import *
from i18n import gettext as _
demandload(globals(), "localrepo bundlerepo httprepo sshrepo statichttprepo")
demandload(globals(), "os util")

def bundle(ui, path):
    if path.startswith('bundle://'):
        path = path[9:]
    else:
        path = path[7:]
    s = path.split("+", 1)
    if len(s) == 1:
        repopath, bundlename = "", s[0]
    else:
        repopath, bundlename = s
    return bundlerepo.bundlerepository(ui, repopath, bundlename)

def hg(ui, path):
    ui.warn(_("hg:// syntax is deprecated, please use http:// instead\n"))
    return httprepo.httprepository(ui, path.replace("hg://", "http://"))

def local_(ui, path, create=0):
    if path.startswith('file:'):
        path = path[5:]
    return localrepo.localrepository(ui, path, create)

def ssh_(ui, path, create=0):
    return sshrepo.sshrepository(ui, path, create)

def old_http(ui, path):
    ui.warn(_("old-http:// syntax is deprecated, "
              "please use static-http:// instead\n"))
    return statichttprepo.statichttprepository(
        ui, path.replace("old-http://", "http://"))

def static_http(ui, path):
    return statichttprepo.statichttprepository(
        ui, path.replace("static-http://", "http://"))

schemes = {
    'bundle': bundle,
    'file': local_,
    'hg': hg,
    'http': lambda ui, path: httprepo.httprepository(ui, path),
    'https': lambda ui, path: httprepo.httpsrepository(ui, path),
    'old-http': old_http,
    'ssh': ssh_,
    'static-http': static_http,
    }

def repository(ui, path=None, create=0):
    if not path: path = ''
    scheme = path
    if scheme:
        c = scheme.find(':')
        scheme = c >= 0 and scheme[:c]
    ctor = schemes.get(scheme) or schemes['file']
    if create:
        try:
            return ctor(ui, path, create)
        except TypeError:
            raise util.Abort(_('cannot create new repository over "%s" protocol') %
                             scheme)
    return ctor(ui, path)
