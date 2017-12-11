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
ispypy = (r'__pypy__' in sys.builtin_module_names)

if not ispy3:
    import cookielib
    import cPickle as pickle
    import httplib
    import Queue as _queue
    import SocketServer as socketserver
    import xmlrpclib
else:
    import http.cookiejar as cookielib
    import http.client as httplib
    import pickle
    import queue as _queue
    import socketserver
    import xmlrpc.client as xmlrpclib

empty = _queue.Empty
queue = _queue.Queue

def identity(a):
    return a

if ispy3:
    import builtins
    import functools
    import io
    import struct

    fsencode = os.fsencode
    fsdecode = os.fsdecode
    oslinesep = os.linesep.encode('ascii')
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
    stringio = io.BytesIO
    maplist = lambda *args: list(map(*args))
    ziplist = lambda *args: list(zip(*args))
    rawinput = input

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
    if getattr(sys, 'argv', None) is not None:
        sysargv = list(map(os.fsencode, sys.argv))

    bytechr = struct.Struct('>B').pack

    class bytestr(bytes):
        """A bytes which mostly acts as a Python 2 str

        >>> bytestr(), bytestr(bytearray(b'foo')), bytestr(u'ascii'), bytestr(1)
        (b'', b'foo', b'ascii', b'1')
        >>> s = bytestr(b'foo')
        >>> assert s is bytestr(s)

        __bytes__() should be called if provided:

        >>> class bytesable(object):
        ...     def __bytes__(self):
        ...         return b'bytes'
        >>> bytestr(bytesable())
        b'bytes'

        There's no implicit conversion from non-ascii str as its encoding is
        unknown:

        >>> bytestr(chr(0x80)) # doctest: +ELLIPSIS
        Traceback (most recent call last):
          ...
        UnicodeEncodeError: ...

        Comparison between bytestr and bytes should work:

        >>> assert bytestr(b'foo') == b'foo'
        >>> assert b'foo' == bytestr(b'foo')
        >>> assert b'f' in bytestr(b'foo')
        >>> assert bytestr(b'f') in b'foo'

        Sliced elements should be bytes, not integer:

        >>> s[1], s[:2]
        (b'o', b'fo')
        >>> list(s), list(reversed(s))
        ([b'f', b'o', b'o'], [b'o', b'o', b'f'])

        As bytestr type isn't propagated across operations, you need to cast
        bytes to bytestr explicitly:

        >>> s = bytestr(b'foo').upper()
        >>> t = bytestr(s)
        >>> s[0], t[0]
        (70, b'F')

        Be careful to not pass a bytestr object to a function which expects
        bytearray-like behavior.

        >>> t = bytes(t)  # cast to bytes
        >>> assert type(t) is bytes
        """

        def __new__(cls, s=b''):
            if isinstance(s, bytestr):
                return s
            if (not isinstance(s, (bytes, bytearray))
                and not hasattr(s, u'__bytes__')):  # hasattr-py3-only
                s = str(s).encode(u'ascii')
            return bytes.__new__(cls, s)

        def __getitem__(self, key):
            s = bytes.__getitem__(self, key)
            if not isinstance(s, bytes):
                s = bytechr(s)
            return s

        def __iter__(self):
            return iterbytestr(bytes.__iter__(self))

    def iterbytestr(s):
        """Iterate bytes as if it were a str object of Python 2"""
        return map(bytechr, s)

    def sysbytes(s):
        """Convert an internal str (e.g. keyword, __doc__) back to bytes

        This never raises UnicodeEncodeError, but only ASCII characters
        can be round-trip by sysstr(sysbytes(s)).
        """
        return s.encode(u'utf-8')

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

    def strurl(url):
        """Converts a bytes url back to str"""
        return url.decode(u'ascii')

    def bytesurl(url):
        """Converts a str url to bytes by encoding in ascii"""
        return url.encode(u'ascii')

    def raisewithtb(exc, tb):
        """Raise exception with the given traceback"""
        raise exc.with_traceback(tb)

    def getdoc(obj):
        """Get docstring as bytes; may be None so gettext() won't confuse it
        with _('')"""
        doc = getattr(obj, u'__doc__', None)
        if doc is None:
            return doc
        return sysbytes(doc)

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
    unicode = str

    def open(name, mode='r', buffering=-1):
        return builtins.open(name, sysstr(mode), buffering)

    def _getoptbwrapper(orig, args, shortlist, namelist):
        """
        Takes bytes arguments, converts them to unicode, pass them to
        getopt.getopt(), convert the returned values back to bytes and then
        return them for Python 3 compatibility as getopt.getopt() don't accepts
        bytes on Python 3.
        """
        args = [a.decode('latin-1') for a in args]
        shortlist = shortlist.decode('latin-1')
        namelist = [a.decode('latin-1') for a in namelist]
        opts, args = orig(args, shortlist, namelist)
        opts = [(a[0].encode('latin-1'), a[1].encode('latin-1'))
                for a in opts]
        args = [a.encode('latin-1') for a in args]
        return opts, args

    def strkwargs(dic):
        """
        Converts the keys of a python dictonary to str i.e. unicodes so that
        they can be passed as keyword arguments as dictonaries with bytes keys
        can't be passed as keyword arguments to functions on Python 3.
        """
        dic = dict((k.decode('latin-1'), v) for k, v in dic.iteritems())
        return dic

    def byteskwargs(dic):
        """
        Converts keys of python dictonaries to bytes as they were converted to
        str to pass that dictonary as a keyword argument on Python 3.
        """
        dic = dict((k.encode('latin-1'), v) for k, v in dic.iteritems())
        return dic

    # TODO: handle shlex.shlex().
    def shlexsplit(s):
        """
        Takes bytes argument, convert it to str i.e. unicodes, pass that into
        shlex.split(), convert the returned value to bytes and return that for
        Python 3 compatibility as shelx.split() don't accept bytes on Python 3.
        """
        ret = shlex.split(s.decode('latin-1'))
        return [a.encode('latin-1') for a in ret]

else:
    import cStringIO

    bytechr = chr
    bytestr = str
    iterbytestr = iter
    sysbytes = identity
    sysstr = identity
    strurl = identity
    bytesurl = identity

    # this can't be parsed on Python 3
    exec('def raisewithtb(exc, tb):\n'
         '    raise exc, None, tb\n')

    def fsencode(filename):
        """
        Partial backport from os.py in Python 3, which only accepts bytes.
        In Python 2, our paths should only ever be bytes, a unicode path
        indicates a bug.
        """
        if isinstance(filename, str):
            return filename
        else:
            raise TypeError(
                "expect str, not %s" % type(filename).__name__)

    # In Python 2, fsdecode() has a very chance to receive bytes. So it's
    # better not to touch Python 2 part as it's already working fine.
    fsdecode = identity

    def getdoc(obj):
        return getattr(obj, '__doc__', None)

    def _getoptbwrapper(orig, args, shortlist, namelist):
        return orig(args, shortlist, namelist)

    strkwargs = identity
    byteskwargs = identity

    oslinesep = os.linesep
    osname = os.name
    ospathsep = os.pathsep
    ossep = os.sep
    osaltsep = os.altsep
    stdin = sys.stdin
    stdout = sys.stdout
    stderr = sys.stderr
    if getattr(sys, 'argv', None) is not None:
        sysargv = sys.argv
    sysplatform = sys.platform
    getcwd = os.getcwd
    sysexecutable = sys.executable
    shlexsplit = shlex.split
    stringio = cStringIO.StringIO
    maplist = map
    ziplist = zip
    rawinput = raw_input

isjython = sysplatform.startswith('java')

isdarwin = sysplatform == 'darwin'
isposix = osname == 'posix'
iswindows = osname == 'nt'

def getoptb(args, shortlist, namelist):
    return _getoptbwrapper(getopt.getopt, args, shortlist, namelist)

def gnugetoptb(args, shortlist, namelist):
    return _getoptbwrapper(getopt.gnu_getopt, args, shortlist, namelist)
