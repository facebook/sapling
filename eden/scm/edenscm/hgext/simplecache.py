# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# simplecache.py - cache slow things locally so they are fast the next time

"""
simplecache is a dirt-simple cache of various functions that get slow in large
repositories. It is aimed at speeding up common operations that programmers
often take, like diffing two revisions (eg, hg export).

Currently we cache the full results of these functions:
    copies.pathcopies (a dictionary)
    context.basectx._buildstatus (a scmutil.status object -- a tuple of lists)

Config::

  [simplecache]
  # enable debug statements (defaults to 'on' except during tests)
  showdebug = False

  # list of caches to enable ('local' or 'memcache')
  caches = local

  # path for local cache files
  cachedir = ~/.hgsimplecache

  # memcache host
  host = localhost

  # memcache port
  port = 11101
"""

import base64
import hashlib
import os
import random
import socket
import tempfile
from typing import Optional, Sized

from edenscm.mercurial import (
    context,
    copies,
    encoding,
    error,
    extensions,
    json,
    node,
    pycompat,
    util,
)
from edenscm.mercurial.node import nullid, wdirid
from edenscm.mercurial.pycompat import range
from edenscm.mercurial.scmutil import status


testedwith = "ships-with-fb-hgext"

# context nodes that are special and should not be cached.
UNCACHEABLE_NODES = [None, nullid, wdirid]  # repo[None].node() returns this

mcroutersocket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)


def extsetup(ui) -> None:
    extensions.wrapfunction(copies, "pathcopies", pathcopiesui(ui))
    extensions.wrapfunction(context.basectx, "_buildstatus", buildstatusui(ui))


def gethostport(ui):
    """
    Return host port to talk to mcrouter.
    """
    host = ui.config("simplecache", "host", default="localhost")
    port = int(ui.config("simplecache", "port", default=11101))
    return (host, port)


def mcget(key: bytes, ui):
    """
    Use local mcrouter to get a key from memcache
    """
    if type(key) != str:
        raise ValueError("Key must be a string")

    key = pycompat.encodeutf8(key)
    key = b"cca.hg.%s" % key

    try:
        mcroutersocket.sendall(b"get %s\r\n" % key)
    except (socket.error, error.SignalInterrupt):
        mcroutersocket.connect(gethostport(ui))
        mcroutersocket.sendall(b"get %s\r\n" % key)

    meta = []
    value = None
    while True:
        char = mcroutersocket.recv(1)

        # No data was received, potentially due to a closed connection, let's
        # consider this a cache-miss and return.
        # XXX: We may want to raise an exception instead.
        if char == b"":
            break

        if char != b"\r":
            meta.append(char)
        else:
            meta = b"".join(meta)
            if meta == b"END":
                break
            char = mcroutersocket.recv(1)  # throw away newline
            _, key, flags, sz = b"".join(meta).strip().split(b" ")
            value = mcroutersocket.recv(int(sz))
            mcroutersocket.recv(7)  # throw away \r\nEND\r\n

            if len(value) != int(sz):
                return None

            break
    return value


def mcset(key, value: bytes, ui) -> bool:
    """
    Use local mcrouter to set a key to memcache
    """
    if type(key) != str:
        raise ValueError("Key must be a string")
    if type(value) != bytes:
        raise ValueError("Value must be bytes")

    key = pycompat.encodeutf8("cca.hg.%s" % key)
    sz = len(value)
    tmpl = b"set %s 0 0 %d\r\n%s\r\n"

    try:
        mcroutersocket.sendall(tmpl % (key, sz, value))
    except (socket.error, error.SignalInterrupt):
        mcroutersocket.connect(gethostport(ui))
        mcroutersocket.sendall(tmpl % (key, sz, value))

    data = []
    while True:
        char = mcroutersocket.recv(1)

        # No data was received, potentially due to a closed connection, let's
        # just return.
        if char == b"":
            return False

        if char not in b"\r\n":
            data.append(char)
        else:
            break
    return b"".join(data) == b"STORED"


class jsonserializer(object):
    """
    Serialize and deserialize simple Python datastructures.

    Any Python object that can be JSON-serialized is fair game.
    """

    @classmethod
    def serialize(cls, input):
        return pycompat.encodeutf8(json.dumps(input))

    @classmethod
    def deserialize(cls, string):
        return json.loads(pycompat.decodeutf8(string))


class pathcopiesserializer(jsonserializer):
    """
    Serialize and deserialize the results of calls to copies.pathcopies.
    Results are just dictionaries, so this just uses json.
    """

    @classmethod
    def serialize(cls, copydict):
        encoded = dict(
            (
                pycompat.decodeutf8(base64.b64encode(pycompat.encodeutf8(k))),
                pycompat.decodeutf8(base64.b64encode(pycompat.encodeutf8(v))),
            )
            for (k, v) in pycompat.iteritems(copydict)
        )
        return super(pathcopiesserializer, cls).serialize(encoded)

    @classmethod
    def deserialize(cls, string):
        encoded = super(pathcopiesserializer, cls).deserialize(string)
        return dict(
            (
                pycompat.decodeutf8(base64.b64decode(pycompat.encodeutf8(k))),
                pycompat.decodeutf8(base64.b64decode(pycompat.encodeutf8(v))),
            )
            for k, v in pycompat.iteritems(encoded)
        )


def pathcopiesui(ui):
    def pathcopies(orig, x, y, match=None):
        func = lambda: orig(x, y, match=match)
        if (
            x.node() not in UNCACHEABLE_NODES
            and y.node() not in UNCACHEABLE_NODES
            and not match
        ):
            key = "pathcopies:%s:%s" % (node.hex(x.node()), node.hex(y.node()))
            return memoize(func, key, pathcopiesserializer, ui)
        return func()

    return pathcopies


class buildstatusserializer(jsonserializer):
    """
    Serialize and deserialize the results of calls to buildstatus.
    Results are status objects, which extend tuple. Each status object
    has seven lists within it, each containing strings of filenames in
    each type of status.
    """

    @classmethod
    def serialize(cls, status):
        ls = [list(status[i]) for i in range(7)]
        ll = []
        for s in ls:
            ll.append(
                [
                    pycompat.decodeutf8(base64.b64encode(pycompat.encodeutf8(f)))
                    for f in s
                ]
            )
        return super(buildstatusserializer, cls).serialize(ll)

    @classmethod
    def deserialize(cls, string):
        ll = super(buildstatusserializer, cls).deserialize(string)
        ls = []
        for l in ll:
            ls.append(
                [
                    pycompat.decodeutf8(base64.b64decode(pycompat.encodeutf8(f)))
                    for f in l
                ]
            )
        return status(*ls)


def buildstatusui(ui):
    def buildstatus(orig, self, other, status, match, ignored, clean, unknown):
        func = lambda: orig(self, other, status, match, ignored, clean, unknown)
        if not match.always():
            return func()
        if ignored or clean or unknown:
            return func()
        if self.node() in UNCACHEABLE_NODES or other.node() in UNCACHEABLE_NODES:
            return func()
        key = "buildstatus:%s:%s" % (node.hex(self.node()), node.hex(other.node()))
        return memoize(func, key, buildstatusserializer, ui)

    return buildstatus


class stringserializer(object):
    """Simple serializer that just checks if the input is a string and returns
    it.
    """

    @staticmethod
    def serialize(input):
        if type(input) is not str:
            raise TypeError("stringserializer can only be used with strings")
        return pycompat.encodeutf8(input)

    @staticmethod
    def deserialize(string):
        if type(string) is not bytes:
            raise TypeError("stringserializer can only be used with strings")
        return pycompat.decodeutf8(string)


def localpath(key, ui) -> str:
    tempdir = ui.config("simplecache", "cachedir")
    if not tempdir:
        tempdir = os.path.join(
            encoding.environ.get("TESTTMP", tempfile.gettempdir()), "hgsimplecache"
        )
    return os.path.join(tempdir, key)


def localget(key: str, ui) -> Optional[bytes]:
    if type(key) != str:
        raise ValueError("Key must be a string")
    try:
        path = localpath(key, ui)
        with open(path, "rb") as f:
            return f.read()
    except Exception:
        return None


def localset(key: str, value: bytes, ui) -> None:
    if type(key) != str:
        raise ValueError("Key must be a string")
    if type(value) != bytes:
        raise ValueError("Value must be bytes")
    try:
        path = localpath(key, ui)
        dirname = os.path.dirname(path)
        if not os.path.exists(dirname):
            os.makedirs(dirname)
        with open(path, "wb") as f:
            f.write(value)

        # If too many entries in cache, delete some.
        tempdirpath = localpath("", ui)
        entries = os.listdir(tempdirpath)
        maxcachesize = ui.configint("simplecache", "maxcachesize", 2000)
        if len(entries) > maxcachesize:
            random.shuffle(entries)
            evictionpercent = ui.configint("simplecache", "evictionpercent", 50)
            evictionpercent /= 100.0
            for i in range(0, int(len(entries) * evictionpercent)):
                os.remove(os.path.join(tempdirpath, entries[i]))
    except Exception:
        return


cachefuncs = {"local": (localget, localset), "memcache": (mcget, mcset)}


def _adjust_key(key: str, ui) -> str:
    version = ui.config("simplecache", "version", default="2")
    key = "%s:v%s" % (key, version)
    if pycompat.iswindows:
        # : is prohibited in Windows filenames, while ! is allowed
        key = key.replace(":", "!")
    return key


def memoize(func, key: str, serializer, ui):
    key = _adjust_key(key, ui)
    sentinel = object()
    result = cacheget(key, serializer, ui, sentinel, _adjusted=True)
    if result is not sentinel:
        return result

    _debug(ui, "falling back for value %s\n" % key)
    value = func()
    cacheset(key, value, serializer, ui, _adjusted=True)
    return value


def cacheget(key: str, serializer, ui, default=None, _adjusted: bool = False):
    if not _adjusted:
        key = _adjust_key(key, ui)
    cachelist = ui.configlist("simplecache", "caches", ["local"])
    for name in cachelist:
        get, set = cachefuncs[name]
        try:
            cacheval = get(key, ui)
            if cacheval is not None:
                _debug(ui, "got value for key %s from %s\n" % (key, name))
                cacheval = verifychecksum(key, cacheval)
                value = serializer.deserialize(cacheval)
                return value
        except Exception as inst:
            _debug(ui, "error getting or deserializing key %s: %s\n" % (key, inst))
        _debug(ui, "no value found for key %s from %s\n" % (key, name))
    return default


def cacheset(key: str, value, serializer, ui, _adjusted: bool = False) -> None:
    if not _adjusted:
        key = _adjust_key(key, ui)
    cachelist = ui.configlist("simplecache", "caches", ["local"])
    for name in cachelist:
        get, set = cachefuncs[name]
        try:
            serialized = serializer.serialize(value)
            checksummed = addchecksum(key, serialized)
            set(key, checksummed, ui)
            _debug(ui, "set value for key %s to %s\n" % (key, name))
        except Exception as inst:
            _debug(ui, "error setting key %s: %s\n" % (key, inst))


def checksum(key, value):
    key = pycompat.encodeutf8(key)
    s = hashlib.sha1(key)
    s.update(value)
    return pycompat.encodeutf8(node.hex(s.digest()))


def addchecksum(key, value):
    return checksum(key, value) + value


def verifychecksum(key, value: Sized):
    if len(value) < 40:
        raise ValueError("simplecache value too short to contain a checksum")

    # pyre-fixme[16]: `Sized` has no attribute `__getitem__`.
    sha, payload = value[:40], value[40:]
    if checksum(key, payload) != sha:
        raise ValueError("invalid hash from simplecache for key '%s'" % key)
    return payload


def _debug(ui, msg) -> None:
    config = ui.configbool("simplecache", "showdebug", None)
    if config is None:
        config = not util.istest()

    if config:
        ui.debug(msg)
