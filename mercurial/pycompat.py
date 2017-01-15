# pycompat.py - portability shim for python 3
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

import getopt
import os
import shlex
import sys

ispy3 = (sys.version_info[0] >= 3)

if not ispy3:
    import cPickle as pickle
    import cStringIO as io
    import httplib
    import Queue as _queue
    import SocketServer as socketserver
    import urlparse
    urlunquote = urlparse.unquote
    import xmlrpclib
else:
    import http.client as httplib
    import io
    import pickle
    import queue as _queue
    import socketserver
    import urllib.parse as urlparse
    urlunquote = urlparse.unquote_to_bytes
    import xmlrpc.client as xmlrpclib

if ispy3:
    import builtins
    import functools
    fsencode = os.fsencode
    fsdecode = os.fsdecode
    # A bytes version of os.name.
    osname = os.name.encode('ascii')
    ospathsep = os.pathsep.encode('ascii')
    ossep = os.sep.encode('ascii')
    osaltsep = os.altsep
    if osaltsep:
        osaltsep = osaltsep.encode('ascii')
    # os.getcwd() on Python 3 returns string, but it has os.getcwdb() which
    # returns bytes.
    getcwd = os.getcwdb
    sysplatform = sys.platform.encode('ascii')
    sysexecutable = sys.executable
    if sysexecutable:
        sysexecutable = os.fsencode(sysexecutable)

    # TODO: .buffer might not exist if std streams were replaced; we'll need
    # a silly wrapper to make a bytes stream backed by a unicode one.
    stdin = sys.stdin.buffer
    stdout = sys.stdout.buffer
    stderr = sys.stderr.buffer

    # Since Python 3 converts argv to wchar_t type by Py_DecodeLocale() on Unix,
    # we can use os.fsencode() to get back bytes argv.
    #
    # https://hg.python.org/cpython/file/v3.5.1/Programs/python.c#l55
    #
    # TODO: On Windows, the native argv is wchar_t, so we'll need a different
    # workaround to simulate the Python 2 (i.e. ANSI Win32 API) behavior.
    sysargv = list(map(os.fsencode, sys.argv))

    def sysstr(s):
        """Return a keyword str to be passed to Python functions such as
        getattr() and str.encode()

        This never raises UnicodeDecodeError. Non-ascii characters are
        considered invalid and mapped to arbitrary but unique code points
        such that 'sysstr(a) != sysstr(b)' for all 'a != b'.
        """
        if isinstance(s, builtins.str):
            return s
        return s.decode(u'latin-1')

    def _wrapattrfunc(f):
        @functools.wraps(f)
        def w(object, name, *args):
            return f(object, sysstr(name), *args)
        return w

    # these wrappers are automagically imported by hgloader
    delattr = _wrapattrfunc(builtins.delattr)
    getattr = _wrapattrfunc(builtins.getattr)
    hasattr = _wrapattrfunc(builtins.hasattr)
    setattr = _wrapattrfunc(builtins.setattr)
    xrange = builtins.range

    # getopt.getopt() on Python 3 deals with unicodes internally so we cannot
    # pass bytes there. Passing unicodes will result in unicodes as return
    # values which we need to convert again to bytes.
    def getoptb(args, shortlist, namelist):
        args = [a.decode('latin-1') for a in args]
        shortlist = shortlist.decode('latin-1')
        namelist = [a.decode('latin-1') for a in namelist]
        opts, args = getopt.getopt(args, shortlist, namelist)
        opts = [(a[0].encode('latin-1'), a[1].encode('latin-1'))
                for a in opts]
        args = [a.encode('latin-1') for a in args]
        return opts, args

    # keys of keyword arguments in Python need to be strings which are unicodes
    # Python 3. This function takes keyword arguments, convert the keys to str.
    def strkwargs(dic):
        dic = dict((k.decode('latin-1'), v) for k, v in dic.iteritems())
        return dic

    # keys of keyword arguments need to be unicode while passing into
    # a function. This function helps us to convert those keys back to bytes
    # again as we need to deal with bytes.
    def byteskwargs(dic):
        dic = dict((k.encode('latin-1'), v) for k, v in dic.iteritems())
        return dic

    # shlex.split() accepts unicodes on Python 3. This function takes bytes
    # argument, convert it into unicodes, pass into shlex.split(), convert the
    # returned value to bytes and return that.
    # TODO: handle shlex.shlex().
    def shlexsplit(s):
        ret = shlex.split(s.decode('latin-1'))
        return [a.encode('latin-1') for a in ret]

else:
    def sysstr(s):
        return s

    # Partial backport from os.py in Python 3, which only accepts bytes.
    # In Python 2, our paths should only ever be bytes, a unicode path
    # indicates a bug.
    def fsencode(filename):
        if isinstance(filename, str):
            return filename
        else:
            raise TypeError(
                "expect str, not %s" % type(filename).__name__)

    # In Python 2, fsdecode() has a very chance to receive bytes. So it's
    # better not to touch Python 2 part as it's already working fine.
    def fsdecode(filename):
        return filename

    def getoptb(args, shortlist, namelist):
        return getopt.getopt(args, shortlist, namelist)

    def strkwargs(dic):
        return dic

    def byteskwargs(dic):
        return dic

    osname = os.name
    ospathsep = os.pathsep
    ossep = os.sep
    osaltsep = os.altsep
    stdin = sys.stdin
    stdout = sys.stdout
    stderr = sys.stderr
    sysargv = sys.argv
    sysplatform = sys.platform
    getcwd = os.getcwd
    sysexecutable = sys.executable
    shlexsplit = shlex.split

stringio = io.StringIO
empty = _queue.Empty
queue = _queue.Queue

class _pycompatstub(object):
    def __init__(self):
        self._aliases = {}

    def _registeraliases(self, origin, items):
        """Add items that will be populated at the first access"""
        items = map(sysstr, items)
        self._aliases.update(
            (item.replace(sysstr('_'), sysstr('')).lower(), (origin, item))
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
