# httprepo.py - HTTP repository proxy classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from remoterepo import *
from i18n import gettext as _
from demandload import *
demandload(globals(), "hg os urllib urllib2 urlparse zlib util httplib")

class passwordmgr(urllib2.HTTPPasswordMgr):
    def __init__(self, ui):
        urllib2.HTTPPasswordMgr.__init__(self)
        self.ui = ui

    def find_user_password(self, realm, authuri):
        authinfo = urllib2.HTTPPasswordMgr.find_user_password(
            self, realm, authuri)
        if authinfo != (None, None):
            return authinfo

        self.ui.write(_("http authorization required\n"))
        self.ui.status(_("realm: %s\n") % realm)
        user = self.ui.prompt(_("user:"), default=None)
        passwd = self.ui.getpass()

        self.add_password(realm, authuri, user, passwd)
        return (user, passwd)

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

        proxyauthinfo = None
        if user and passwd:
            passmgr = urllib2.HTTPPasswordMgrWithDefaultRealm()
            passmgr.add_password(None, host, user, passwd)
            proxyauthinfo = urllib2.ProxyBasicAuthHandler(passmgr)

        if ui.interactive:
            passmgr = passwordmgr(ui)
            opener = urllib2.build_opener(
                proxy_handler, proxyauthinfo,
                urllib2.HTTPBasicAuthHandler(passmgr),
                urllib2.HTTPDigestAuthHandler(passmgr))
        else:
            opener = urllib2.build_opener(proxy_handler, proxyauthinfo)

        # 1.0 here is the _protocol_ version
        opener.addheaders = [('User-agent', 'mercurial/proto-1.0')]
        urllib2.install_opener(opener)

    def dev(self):
        return -1

    def lock(self):
        raise util.Abort(_('operation not supported over http'))

    def do_cmd(self, cmd, **args):
        self.ui.debug(_("sending %s command\n") % cmd)
        q = {"cmd": cmd}
        q.update(args)
        qs = urllib.urlencode(q)
        cu = "%s?%s" % (self.url, qs)
        try:
            resp = urllib2.urlopen(cu)
        except httplib.HTTPException, inst:
            self.ui.debug(_('http error while sending %s command\n') % cmd)
            self.ui.print_exc()
            raise IOError(None, inst)
        proto = resp.headers['content-type']

        # accept old "text/plain" and "application/hg-changegroup" for now
        if not proto.startswith('application/mercurial') and \
               not proto.startswith('text/plain') and \
               not proto.startswith('application/hg-changegroup'):
            raise hg.RepoError(_("'%s' does not appear to be an hg repository") %
                               self.url)

        if proto.startswith('application/mercurial'):
            version = proto[22:]
            if float(version) > 0.1:
                raise hg.RepoError(_("'%s' uses newer protocol %s") %
                                   (self.url, version))

        return resp

    def heads(self):
        d = self.do_cmd("heads").read()
        try:
            return map(bin, d[:-1].split(" "))
        except:
            self.ui.warn(_("unexpected response:\n") + d[:400] + "\n...\n")
            raise

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self.do_cmd("branches", nodes=n).read()
        try:
            br = [ tuple(map(bin, b.split(" "))) for b in d.splitlines() ]
            return br
        except:
            self.ui.warn(_("unexpected response:\n") + d[:400] + "\n...\n")
            raise

    def between(self, pairs):
        n = "\n".join(["-".join(map(hex, p)) for p in pairs])
        d = self.do_cmd("between", pairs=n).read()
        try:
            p = [ l and map(bin, l.split(" ")) or [] for l in d.splitlines() ]
            return p
        except:
            self.ui.warn(_("unexpected response:\n") + d[:400] + "\n...\n")
            raise

    def changegroup(self, nodes, kind):
        n = " ".join(map(hex, nodes))
        f = self.do_cmd("changegroup", roots=n)
        bytes = 0

        def zgenerator(f):
            zd = zlib.decompressobj()
            try:
                for chnk in f:
                    yield zd.decompress(chnk)
            except httplib.HTTPException, inst:
                raise IOError(None, _('connection ended unexpectedly'))
            yield zd.flush()

        return util.chunkbuffer(zgenerator(util.filechunkiter(f)))

class httpsrepository(httprepository):
    pass
