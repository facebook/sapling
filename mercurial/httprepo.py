# httprepo.py - HTTP repository proxy classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from remoterepo import *
from demandload import *
demandload(globals(), "hg os urllib urllib2 urlparse zlib")

class httprepository(remoterepository):
    def __init__(self, ui, path):
        # fix missing / after hostname
        s = urlparse.urlsplit(path)
        partial = s[2]
        if not partial: partial = "/"
        self.url = urlparse.urlunsplit((s[0], s[1], partial, '', ''))
        self.ui = ui
        no_list = [ "localhost", "127.0.0.1" ]
        host = ui.config("http_proxy", "host")
        if host is None:
            host = os.environ.get("http_proxy")
        if host and host.startswith('http://'):
            host = host[7:]
        user = ui.config("http_proxy", "user")
        passwd = ui.config("http_proxy", "passwd")
        no = ui.config("http_proxy", "no")
        if no is None:
            no = os.environ.get("no_proxy")
        if no:
            no_list = no_list + no.split(",")

        no_proxy = 0
        for h in no_list:
            if (path.startswith("http://" + h + "/") or
                path.startswith("http://" + h + ":") or
                path == "http://" + h):
                no_proxy = 1

        # Note: urllib2 takes proxy values from the environment and those will
        # take precedence
        for env in ["HTTP_PROXY", "http_proxy", "no_proxy"]:
            try:
                if os.environ.has_key(env):
                    del os.environ[env]
            except OSError:
                pass

        proxy_handler = urllib2.BaseHandler()
        if host and not no_proxy:
            proxy_handler = urllib2.ProxyHandler({"http" : "http://" + host})

        authinfo = None
        if user and passwd:
            passmgr = urllib2.HTTPPasswordMgrWithDefaultRealm()
            passmgr.add_password(None, host, user, passwd)
            authinfo = urllib2.ProxyBasicAuthHandler(passmgr)

        opener = urllib2.build_opener(proxy_handler, authinfo)
        urllib2.install_opener(opener)

    def dev(self):
        return -1

    def do_cmd(self, cmd, **args):
        self.ui.debug("sending %s command\n" % cmd)
        q = {"cmd": cmd}
        q.update(args)
        qs = urllib.urlencode(q)
        cu = "%s?%s" % (self.url, qs)
        resp = urllib2.urlopen(cu)
        proto = resp.headers['content-type']

        # accept old "text/plain" and "application/hg-changegroup" for now
        if not proto.startswith('application/mercurial') and \
               not proto.startswith('text/plain') and \
               not proto.startswith('application/hg-changegroup'):
            raise hg.RepoError("'%s' does not appear to be an hg repository" %
                               self.url)

        if proto.startswith('application/mercurial'):
            version = proto[22:]
            if float(version) > 0.1:
                raise hg.RepoError("'%s' uses newer protocol %s" %
                                   (self.url, version))

        return resp

    def heads(self):
        d = self.do_cmd("heads").read()
        try:
            return map(bin, d[:-1].split(" "))
        except:
            self.ui.warn("unexpected response:\n" + d[:400] + "\n...\n")
            raise

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self.do_cmd("branches", nodes=n).read()
        try:
            br = [ tuple(map(bin, b.split(" "))) for b in d.splitlines() ]
            return br
        except:
            self.ui.warn("unexpected response:\n" + d[:400] + "\n...\n")
            raise

    def between(self, pairs):
        n = "\n".join(["-".join(map(hex, p)) for p in pairs])
        d = self.do_cmd("between", pairs=n).read()
        try:
            p = [ l and map(bin, l.split(" ")) or [] for l in d.splitlines() ]
            return p
        except:
            self.ui.warn("unexpected response:\n" + d[:400] + "\n...\n")
            raise

    def changegroup(self, nodes):
        n = " ".join(map(hex, nodes))
        f = self.do_cmd("changegroup", roots=n)
        bytes = 0

        class zread:
            def __init__(self, f):
                self.zd = zlib.decompressobj()
                self.f = f
                self.buf = ""
            def read(self, l):
                while l > len(self.buf):
                    r = self.f.read(4096)
                    if r:
                        self.buf += self.zd.decompress(r)
                    else:
                        self.buf += self.zd.flush()
                        break
                d, self.buf = self.buf[:l], self.buf[l:]
                return d

        return zread(f)

class httpsrepository(httprepository):
    pass
