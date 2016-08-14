# pycompat.py - portability shim for python 3
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

import sys

if sys.version_info[0] < 3:
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

if sys.version_info[0] >= 3:
    import builtins
    import functools
    builtins.xrange = range

    def _wrapattrfunc(f):
        @functools.wraps(f)
        def w(object, name, *args):
            if isinstance(name, bytes):
                name = name.decode(u'utf-8')
            return f(object, name, *args)
        return w

    delattr = _wrapattrfunc(builtins.delattr)
    getattr = _wrapattrfunc(builtins.getattr)
    hasattr = _wrapattrfunc(builtins.hasattr)
    setattr = _wrapattrfunc(builtins.setattr)

stringio = io.StringIO
empty = _queue.Empty
queue = _queue.Queue

class _pycompatstub(object):
    pass

def _alias(alias, origin, items):
    """ populate a _pycompatstub

    copies items from origin to alias
    """
    for item in items:
        try:
            lcase = item.replace('_', '').lower()
            setattr(alias, lcase, getattr(origin, item))
        except AttributeError:
            pass

httpserver = _pycompatstub()
urlreq = _pycompatstub()
urlerr = _pycompatstub()
try:
    import BaseHTTPServer
    import CGIHTTPServer
    import SimpleHTTPServer
    import urllib2
    import urllib
    _alias(urlreq, urllib, (
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
    _alias(urlreq, urllib2, (
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
    _alias(urlerr, urllib2, (
        "HTTPError",
        "URLError",
    ))
    _alias(httpserver, BaseHTTPServer, (
        "HTTPServer",
        "BaseHTTPRequestHandler",
    ))
    _alias(httpserver, SimpleHTTPServer, (
        "SimpleHTTPRequestHandler",
    ))
    _alias(httpserver, CGIHTTPServer, (
        "CGIHTTPRequestHandler",
    ))

except ImportError:
    import urllib.request
    _alias(urlreq, urllib.request, (
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
    _alias(urlerr, urllib.error, (
        "HTTPError",
        "URLError",
    ))
    import http.server
    _alias(httpserver, http.server, (
        "HTTPServer",
        "BaseHTTPRequestHandler",
        "SimpleHTTPRequestHandler",
        "CGIHTTPRequestHandler",
    ))
