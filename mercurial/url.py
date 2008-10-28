# url.py - HTTP handling for mercurial
#
# Copyright 2005, 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
# Copyright 2006, 2007 Alexis S. L. Carvalho <alexis@cecm.usp.br>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import urllib, urllib2, urlparse, httplib, os, re
from i18n import _
import keepalive, util

def hidepassword(url):
    '''hide user credential in a url string'''
    scheme, netloc, path, params, query, fragment = urlparse.urlparse(url)
    netloc = re.sub('([^:]*):([^@]*)@(.*)', r'\1:***@\3', netloc)
    return urlparse.urlunparse((scheme, netloc, path, params, query, fragment))

def removeauth(url):
    '''remove all authentication information from a url string'''
    scheme, netloc, path, params, query, fragment = urlparse.urlparse(url)
    netloc = netloc[netloc.find('@')+1:]
    return urlparse.urlunparse((scheme, netloc, path, params, query, fragment))

def netlocsplit(netloc):
    '''split [user[:passwd]@]host[:port] into 4-tuple.'''

    a = netloc.find('@')
    if a == -1:
        user, passwd = None, None
    else:
        userpass, netloc = netloc[:a], netloc[a+1:]
        c = userpass.find(':')
        if c == -1:
            user, passwd = urllib.unquote(userpass), None
        else:
            user = urllib.unquote(userpass[:c])
            passwd = urllib.unquote(userpass[c+1:])
    c = netloc.find(':')
    if c == -1:
        host, port = netloc, None
    else:
        host, port = netloc[:c], netloc[c+1:]
    return host, port, user, passwd

def netlocunsplit(host, port, user=None, passwd=None):
    '''turn host, port, user, passwd into [user[:passwd]@]host[:port].'''
    if port:
        hostport = host + ':' + port
    else:
        hostport = host
    if user:
        if passwd:
            userpass = urllib.quote(user) + ':' + urllib.quote(passwd)
        else:
            userpass = urllib.quote(user)
        return userpass + '@' + hostport
    return hostport

_safe = ('abcdefghijklmnopqrstuvwxyz'
         'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
         '0123456789' '_.-/')
_safeset = None
_hex = None
def quotepath(path):
    '''quote the path part of a URL

    This is similar to urllib.quote, but it also tries to avoid
    quoting things twice (inspired by wget):

    >>> quotepath('abc def')
    'abc%20def'
    >>> quotepath('abc%20def')
    'abc%20def'
    >>> quotepath('abc%20 def')
    'abc%20%20def'
    >>> quotepath('abc def%20')
    'abc%20def%20'
    >>> quotepath('abc def%2')
    'abc%20def%252'
    >>> quotepath('abc def%')
    'abc%20def%25'
    '''
    global _safeset, _hex
    if _safeset is None:
        _safeset = util.set(_safe)
        _hex = util.set('abcdefABCDEF0123456789')
    l = list(path)
    for i in xrange(len(l)):
        c = l[i]
        if c == '%' and i + 2 < len(l) and (l[i+1] in _hex and l[i+2] in _hex):
            pass
        elif c not in _safeset:
            l[i] = '%%%02X' % ord(c)
    return ''.join(l)

class passwordmgr(urllib2.HTTPPasswordMgrWithDefaultRealm):
    def __init__(self, ui):
        urllib2.HTTPPasswordMgrWithDefaultRealm.__init__(self)
        self.ui = ui

    def find_user_password(self, realm, authuri):
        authinfo = urllib2.HTTPPasswordMgrWithDefaultRealm.find_user_password(
            self, realm, authuri)
        user, passwd = authinfo
        if user and passwd:
            return (user, passwd)

        if not self.ui.interactive:
            raise util.Abort(_('http authorization required'))

        self.ui.write(_("http authorization required\n"))
        self.ui.status(_("realm: %s\n") % realm)
        if user:
            self.ui.status(_("user: %s\n") % user)
        else:
            user = self.ui.prompt(_("user:"), default=None)

        if not passwd:
            passwd = self.ui.getpass()

        self.add_password(realm, authuri, user, passwd)
        return (user, passwd)

class proxyhandler(urllib2.ProxyHandler):
    def __init__(self, ui):
        proxyurl = ui.config("http_proxy", "host") or os.getenv('http_proxy')
        # XXX proxyauthinfo = None

        if proxyurl:
            # proxy can be proper url or host[:port]
            if not (proxyurl.startswith('http:') or
                    proxyurl.startswith('https:')):
                proxyurl = 'http://' + proxyurl + '/'
            snpqf = urlparse.urlsplit(proxyurl)
            proxyscheme, proxynetloc, proxypath, proxyquery, proxyfrag = snpqf
            hpup = netlocsplit(proxynetloc)

            proxyhost, proxyport, proxyuser, proxypasswd = hpup
            if not proxyuser:
                proxyuser = ui.config("http_proxy", "user")
                proxypasswd = ui.config("http_proxy", "passwd")

            # see if we should use a proxy for this url
            no_list = [ "localhost", "127.0.0.1" ]
            no_list.extend([p.lower() for
                            p in ui.configlist("http_proxy", "no")])
            no_list.extend([p.strip().lower() for
                            p in os.getenv("no_proxy", '').split(',')
                            if p.strip()])
            # "http_proxy.always" config is for running tests on localhost
            if ui.configbool("http_proxy", "always"):
                self.no_list = []
            else:
                self.no_list = no_list

            proxyurl = urlparse.urlunsplit((
                proxyscheme, netlocunsplit(proxyhost, proxyport,
                                                proxyuser, proxypasswd or ''),
                proxypath, proxyquery, proxyfrag))
            proxies = {'http': proxyurl, 'https': proxyurl}
            ui.debug(_('proxying through http://%s:%s\n') %
                      (proxyhost, proxyport))
        else:
            proxies = {}

        # urllib2 takes proxy values from the environment and those
        # will take precedence if found, so drop them
        for env in ["HTTP_PROXY", "http_proxy", "no_proxy"]:
            try:
                if env in os.environ:
                    del os.environ[env]
            except OSError:
                pass

        urllib2.ProxyHandler.__init__(self, proxies)
        self.ui = ui

    def proxy_open(self, req, proxy, type_):
        host = req.get_host().split(':')[0]
        if host in self.no_list:
            return None

        # work around a bug in Python < 2.4.2
        # (it leaves a "\n" at the end of Proxy-authorization headers)
        baseclass = req.__class__
        class _request(baseclass):
            def add_header(self, key, val):
                if key.lower() == 'proxy-authorization':
                    val = val.strip()
                return baseclass.add_header(self, key, val)
        req.__class__ = _request

        return urllib2.ProxyHandler.proxy_open(self, req, proxy, type_)

class httpsendfile(file):
    def __len__(self):
        return os.fstat(self.fileno()).st_size

def _gen_sendfile(connection):
    def _sendfile(self, data):
        # send a file
        if isinstance(data, httpsendfile):
            # if auth required, some data sent twice, so rewind here
            data.seek(0)
            for chunk in util.filechunkiter(data):
                connection.send(self, chunk)
        else:
            connection.send(self, data)
    return _sendfile

class httpconnection(keepalive.HTTPConnection):
    # must be able to send big bundle as stream.
    send = _gen_sendfile(keepalive.HTTPConnection)

class httphandler(keepalive.HTTPHandler):
    def http_open(self, req):
        return self.do_open(httpconnection, req)

    def __del__(self):
        self.close_all()

has_https = hasattr(urllib2, 'HTTPSHandler')
if has_https:
    class httpsconnection(httplib.HTTPSConnection):
        response_class = keepalive.HTTPResponse
        # must be able to send big bundle as stream.
        send = _gen_sendfile(httplib.HTTPSConnection)

    class httpshandler(keepalive.KeepAliveHandler, urllib2.HTTPSHandler):
        def https_open(self, req):
            return self.do_open(httpsconnection, req)

# In python < 2.5 AbstractDigestAuthHandler raises a ValueError if
# it doesn't know about the auth type requested.  This can happen if
# somebody is using BasicAuth and types a bad password.
class httpdigestauthhandler(urllib2.HTTPDigestAuthHandler):
    def http_error_auth_reqed(self, auth_header, host, req, headers):
        try:
            return urllib2.HTTPDigestAuthHandler.http_error_auth_reqed(
                        self, auth_header, host, req, headers)
        except ValueError, inst:
            arg = inst.args[0]
            if arg.startswith("AbstractDigestAuthHandler doesn't know "):
                return
            raise

def getauthinfo(path):
    scheme, netloc, urlpath, query, frag = urlparse.urlsplit(path)
    if not urlpath:
        urlpath = '/'
    if scheme != 'file':
        # XXX: why are we quoting the path again with some smart
        # heuristic here? Anyway, it cannot be done with file://
        # urls since path encoding is os/fs dependent (see
        # urllib.pathname2url() for details).
        urlpath = quotepath(urlpath)
    host, port, user, passwd = netlocsplit(netloc)

    # urllib cannot handle URLs with embedded user or passwd
    url = urlparse.urlunsplit((scheme, netlocunsplit(host, port),
                              urlpath, query, frag))
    if user:
        netloc = host
        if port:
            netloc += ':' + port
        # Python < 2.4.3 uses only the netloc to search for a password
        authinfo = (None, (url, netloc), user, passwd or '')
    else:
        authinfo = None
    return url, authinfo

def opener(ui, authinfo=None):
    '''
    construct an opener suitable for urllib2
    authinfo will be added to the password manager
    '''
    handlers = [httphandler()]
    if has_https:
        handlers.append(httpshandler())

    handlers.append(proxyhandler(ui))

    passmgr = passwordmgr(ui)
    if authinfo is not None:
        passmgr.add_password(*authinfo)
        user, passwd = authinfo[2:4]
        ui.debug(_('http auth: user %s, password %s\n') %
                 (user, passwd and '*' * len(passwd) or 'not set'))

    handlers.extend((urllib2.HTTPBasicAuthHandler(passmgr),
                     httpdigestauthhandler(passmgr)))
    opener = urllib2.build_opener(*handlers)

    # 1.0 here is the _protocol_ version
    opener.addheaders = [('User-agent', 'mercurial/proto-1.0')]
    opener.addheaders.append(('Accept', 'application/mercurial-0.1'))
    return opener

scheme_re = re.compile(r'^([a-zA-Z0-9+-.]+)://')

def open(ui, url, data=None):
    scheme = None
    m = scheme_re.search(url)
    if m:
        scheme = m.group(1).lower()
    if not scheme:
        path = util.normpath(os.path.abspath(url))
        url = 'file://' + urllib.pathname2url(path)
        authinfo = None
    else:
        url, authinfo = getauthinfo(url)
    return opener(ui, authinfo).open(url, data)
