# urllibcompat.py - adapters to ease using urllib2 on Py2 and urllib on Py3
#
# Copyright 2017 Google, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

from . import pycompat

_sysstr = pycompat.sysstr

class _pycompatstub(object):
    def __init__(self):
        self._aliases = {}

    def _registeraliases(self, origin, items):
        """Add items that will be populated at the first access"""
        items = map(_sysstr, items)
        self._aliases.update(
            (item.replace(_sysstr('_'), _sysstr('')).lower(), (origin, item))
            for item in items)

    def _registeralias(self, origin, attr, name):
        """Alias ``origin``.``attr`` as ``name``"""
        self._aliases[_sysstr(name)] = (origin, _sysstr(attr))

    def __getattr__(self, name):
        try:
            origin, item = self._aliases[name]
        except KeyError:
            raise AttributeError(name)
        self.__dict__[name] = obj = getattr(origin, item)
        return obj

httpserver = _pycompatstub()
urlreq = _pycompatstub()
urlerr = _pycompatstub()

if pycompat.ispy3:
    import urllib.parse
    urlreq._registeraliases(urllib.parse, (
        "splitattr",
        "splitpasswd",
        "splitport",
        "splituser",
        "urlparse",
        "urlunparse",
    ))
    urlreq._registeralias(urllib.parse, "unquote_to_bytes", "unquote")
    import urllib.request
    urlreq._registeraliases(urllib.request, (
        "AbstractHTTPHandler",
        "BaseHandler",
        "build_opener",
        "FileHandler",
        "FTPHandler",
        "ftpwrapper",
        "HTTPHandler",
        "HTTPSHandler",
        "install_opener",
        "pathname2url",
        "HTTPBasicAuthHandler",
        "HTTPDigestAuthHandler",
        "HTTPPasswordMgrWithDefaultRealm",
        "ProxyHandler",
        "Request",
        "url2pathname",
        "urlopen",
    ))
    import urllib.response
    urlreq._registeraliases(urllib.response, (
        "addclosehook",
        "addinfourl",
    ))
    import urllib.error
    urlerr._registeraliases(urllib.error, (
        "HTTPError",
        "URLError",
    ))
    import http.server
    httpserver._registeraliases(http.server, (
        "HTTPServer",
        "BaseHTTPRequestHandler",
        "SimpleHTTPRequestHandler",
        "CGIHTTPRequestHandler",
    ))

    # urllib.parse.quote() accepts both str and bytes, decodes bytes
    # (if necessary), and returns str. This is wonky. We provide a custom
    # implementation that only accepts bytes and emits bytes.
    def quote(s, safe=r'/'):
        s = urllib.parse.quote_from_bytes(s, safe=safe)
        return s.encode('ascii', 'strict')

    # urllib.parse.urlencode() returns str. We use this function to make
    # sure we return bytes.
    def urlencode(query, doseq=False):
            s = urllib.parse.urlencode(query, doseq=doseq)
            return s.encode('ascii')

    urlreq.quote = quote
    urlreq.urlencode = urlencode

    def getfullurl(req):
        return req.full_url

    def gethost(req):
        return req.host

    def getselector(req):
        return req.selector

    def getdata(req):
        return req.data

    def hasdata(req):
        return req.data is not None
else:
    import BaseHTTPServer
    import CGIHTTPServer
    import SimpleHTTPServer
    import urllib2
    import urllib
    import urlparse
    urlreq._registeraliases(urllib, (
        "addclosehook",
        "addinfourl",
        "ftpwrapper",
        "pathname2url",
        "quote",
        "splitattr",
        "splitpasswd",
        "splitport",
        "splituser",
        "unquote",
        "url2pathname",
        "urlencode",
    ))
    urlreq._registeraliases(urllib2, (
        "AbstractHTTPHandler",
        "BaseHandler",
        "build_opener",
        "FileHandler",
        "FTPHandler",
        "HTTPBasicAuthHandler",
        "HTTPDigestAuthHandler",
        "HTTPHandler",
        "HTTPPasswordMgrWithDefaultRealm",
        "HTTPSHandler",
        "install_opener",
        "ProxyHandler",
        "Request",
        "urlopen",
    ))
    urlreq._registeraliases(urlparse, (
        "urlparse",
        "urlunparse",
    ))
    urlerr._registeraliases(urllib2, (
        "HTTPError",
        "URLError",
    ))
    httpserver._registeraliases(BaseHTTPServer, (
        "HTTPServer",
        "BaseHTTPRequestHandler",
    ))
    httpserver._registeraliases(SimpleHTTPServer, (
        "SimpleHTTPRequestHandler",
    ))
    httpserver._registeraliases(CGIHTTPServer, (
        "CGIHTTPRequestHandler",
    ))

    def gethost(req):
        return req.get_host()

    def getselector(req):
        return req.get_selector()

    def getfullurl(req):
        return req.get_full_url()

    def getdata(req):
        return req.get_data()

    def hasdata(req):
        return req.has_data()
