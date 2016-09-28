# pycompat.py - portability shim for python 3
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

import sys

ispy3 = (sys.version_info[0] >= 3)

if not ispy3:
    import cPickle as pickle
    import cStringIO as io
    import httplib
    import Queue as _queue
    import SocketServer as socketserver
    import urlparse
    import xmlrpclib
else:
    import http.client as httplib
    import io
    import pickle
    import queue as _queue
    import socketserver
    import urllib.parse as urlparse
    import xmlrpc.client as xmlrpclib

if ispy3:
    import builtins
    import functools

    def _wrapattrfunc(f):
        @functools.wraps(f)
        def w(object, name, *args):
            if isinstance(name, bytes):
                name = name.decode(u'utf-8')
            return f(object, name, *args)
        return w

    # these wrappers are automagically imported by hgloader
    delattr = _wrapattrfunc(builtins.delattr)
    getattr = _wrapattrfunc(builtins.getattr)
    hasattr = _wrapattrfunc(builtins.hasattr)
    setattr = _wrapattrfunc(builtins.setattr)
    xrange = builtins.range

stringio = io.StringIO
empty = _queue.Empty
queue = _queue.Queue

class _pycompatstub(object):
    def __init__(self):
        self._aliases = {}

    def _registeraliases(self, origin, items):
        """Add items that will be populated at the first access"""
        self._aliases.update((item.replace('_', '').lower(), (origin, item))
                             for item in items)

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
if not ispy3:
    import BaseHTTPServer
    import CGIHTTPServer
    import SimpleHTTPServer
    import urllib2
    import urllib
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

else:
    import urllib.request
    urlreq._registeraliases(urllib.request, (
        "AbstractHTTPHandler",
        "addclosehook",
        "addinfourl",
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
        "quote",
        "Request",
        "splitattr",
        "splitpasswd",
        "splitport",
        "splituser",
        "unquote",
        "url2pathname",
        "urlopen",
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
