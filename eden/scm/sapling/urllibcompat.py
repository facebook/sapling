# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# urllibcompat.py - adapters to ease using urllib2 on Py2 and urllib on Py3
#
# Copyright 2017 Google, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

from . import pycompat


class _pycompatstub:
    def __init__(self):
        self._aliases = {}

    def _registeraliases(self, origin, items):
        """Add items that will be populated at the first access"""
        self._aliases.update(
            (item.replace("_", "").lower(), (origin, item)) for item in items
        )

    def _registeralias(self, origin, attr, name):
        """Alias ``origin``.``attr`` as ``name``"""
        self._aliases[name] = (origin, attr)

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


import urllib.parse

urlreq._registeraliases(
    urllib.parse,
    (
        "splitattr",
        "splitpasswd",
        "splitport",
        "splituser",
        "urlparse",
        "urlunparse",
    ),
)
urlreq._registeralias(urllib.parse, "unquote", "unquote")
import urllib.request

urlreq._registeraliases(
    urllib.request,
    (
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
    ),
)
import urllib.response

urlreq._registeraliases(urllib.response, ("addclosehook", "addinfourl"))
import urllib.error

urlerr._registeraliases(urllib.error, ("HTTPError", "URLError"))
import http.server

httpserver._registeraliases(
    http.server,
    (
        "HTTPServer",
        "BaseHTTPRequestHandler",
        "SimpleHTTPRequestHandler",
        "CGIHTTPRequestHandler",
    ),
)

# quote() and unquote() both operate on and return strings (not bytes)
quote = urllib.parse.quote
unquote = urllib.parse.unquote

# urllib.parse.urlencode() returns str. We use this function to make
# sure we return bytes.
def urlencode(query, doseq: bool = False):
    s = pycompat.encodeutf8(urllib.parse.urlencode(query, doseq=doseq))
    return s


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


def hasdata(req) -> bool:
    return req.data is not None
