# zeroconf.py - zeroconf support for Mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License (version 2), incorporated herein by
# reference.

import Zeroconf, socket, time, os
from mercurial import ui
from mercurial import extensions
from mercurial.hgweb import hgweb_mod
from mercurial.hgweb import hgwebdir_mod

# publish

server = None
localip = None

def getip():
    # finds external-facing interface without sending any packets (Linux)
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(('1.0.0.1', 0))
        ip = s.getsockname()[0]
        return ip
    except:
        pass

    # Generic method, sometimes gives useless results
    dumbip = socket.gethostbyaddr(socket.gethostname())[2][0]
    if not dumbip.startswith('127.'):
        return dumbip

    # works elsewhere, but actually sends a packet
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(('1.0.0.1', 1))
        ip = s.getsockname()[0]
        return ip
    except:
        pass

    return dumbip

def publish(name, desc, path, port):
    global server, localip
    if not server:
        server = Zeroconf.Zeroconf()
        ip = getip()
        localip = socket.inet_aton(ip)

    parts = socket.gethostname().split('.')
    host = parts[0] + ".local"

    # advertise to browsers
    svc = Zeroconf.ServiceInfo('_http._tcp.local.',
                               name + '._http._tcp.local.',
                               server = host,
                               port = port,
                               properties = {'description': desc,
                                             'path': "/" + path},
                               address = localip, weight = 0, priority = 0)
    server.registerService(svc)

    # advertise to Mercurial clients
    svc = Zeroconf.ServiceInfo('_hg._tcp.local.',
                               name + '._hg._tcp.local.',
                               server = host,
                               port = port,
                               properties = {'description': desc,
                                             'path': "/" + path},
                               address = localip, weight = 0, priority = 0)
    server.registerService(svc)

class hgwebzc(hgweb_mod.hgweb):
    def __init__(self, repo, name=None):
        super(hgwebzc, self).__init__(repo, name)
        name = self.reponame or os.path.basename(repo.root)
        desc = self.repo.ui.config("web", "description", name)
        publish(name, desc, name, int(repo.ui.config("web", "port", 8000)))

class hgwebdirzc(hgwebdir_mod.hgwebdir):
    def run(self):
        print os.environ
        for r, p in self.repos:
            u = ui.ui(parentui=self.parentui)
            u.readconfig(os.path.join(path, '.hg', 'hgrc'))
            n = os.path.basename(r)
            desc = u.config("web", "description", n)
            publish(n, "hgweb", p, int(repo.ui.config("web", "port", 8000)))
        return super(hgwebdirzc, self).run()

# listen

class listener(object):
    def __init__(self):
        self.found = {}
    def removeService(self, server, type, name):
        if repr(name) in self.found:
            del self.found[repr(name)]
    def addService(self, server, type, name):
        self.found[repr(name)] = server.getServiceInfo(type, name)

def getzcpaths():
    server = Zeroconf.Zeroconf()
    l = listener()
    browser = Zeroconf.ServiceBrowser(server, "_hg._tcp.local.", l)
    time.sleep(1)
    server.close()
    for v in l.found.values():
        n = v.name[:v.name.index('.')]
        n.replace(" ", "-")
        u = "http://%s:%s%s" % (socket.inet_ntoa(v.address), v.port,
                                 v.properties.get("path", "/"))
        yield "zc-" + n, u

def config(orig, self, section, key, default=None, untrusted=False):
    if section == "paths" and key.startswith("zc-"):
        for n, p in getzcpaths():
            if n == key:
                return p
    return orig(self, section, key, default, untrusted)

def configitems(orig, self, section):
    r = orig(self, section, untrusted=False)
    if section == "paths":
        r += getzcpaths()
    return r

extensions.wrapfunction(ui.ui, 'config', config)
extensions.wrapfunction(ui.ui, 'configitems', configitems)
hgweb_mod.hgweb = hgwebzc
hgwebdir_mod.hgwebdir = hgwebdirzc
