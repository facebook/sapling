# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# json.py - json encoding
#
# Copyright 2005-2008 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import json as _sysjson
import sys

from edenscm.mercurial import encoding, pycompat, util


try:
    long
except NameError:
    long = int


def dumps(obj, paranoid=True):
    if obj is None:
        return "null"
    elif obj is False:
        return "false"
    elif obj is True:
        return "true"
    elif isinstance(obj, (int, long, float)):
        return pycompat.bytestr(obj)
    elif isinstance(obj, bytes):
        return '"%s"' % encoding.jsonescape(obj, paranoid=paranoid)
    elif isinstance(obj, str):
        return _sysjson.dumps(obj)
    elif util.safehasattr(obj, "keys"):
        out = []
        for k, v in sorted(pycompat.iteritems(obj)):
            if isinstance(k, bytes):
                key = '"%s"' % encoding.jsonescape(k, paranoid=paranoid)
            else:
                key = _sysjson.dumps(k)
            out.append(key + ": %s" % dumps(v, paranoid))
        return "{" + ", ".join(out) + "}"
    elif util.safehasattr(obj, "__iter__"):
        out = [dumps(i, paranoid) for i in obj]
        return "[" + ", ".join(out) + "]"
    else:
        raise TypeError("cannot encode type %s" % obj.__class__.__name__)


def dump(data, fp):
    if sys.version_info[0] < 3:
        return fp.write(dumps(data))
    else:
        return _sysjson.dump(data, fp)


def _rapply(f, xs):
    if xs is None:
        # assume None means non-value of optional data
        return xs
    if isinstance(xs, (list, set, tuple)):
        return type(xs)(_rapply(f, x) for x in xs)
    if isinstance(xs, dict):
        return type(xs)((_rapply(f, k), _rapply(f, v)) for k, v in xs.items())
    return f(xs)


def load(fp):
    if sys.version_info[0] < 3:
        return loads(fp.read())
    else:
        return _sysjson.load(fp)


def loads(string):
    """Like stdlib json.loads, but results are bytes instead of unicode

    Warning: this does not round-trip with "dumps". "dumps" supports non-utf8
    binary content that is unsupported by this function.
    """
    if sys.version_info[0] < 3:
        # XXX: This should round-trip with "dumps". But it might be non-trivial to
        # do so.
        def encode(s):
            if isinstance(s, type("")):
                return pycompat.decodeutf8(s.encode("utf-8"))
            else:
                return s

        return _rapply(encode, _sysjson.loads(string))
    else:
        return _sysjson.loads(string)
