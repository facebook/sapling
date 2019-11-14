# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# json.py - json encoding
#
# Copyright 2005-2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import json as _sysjson

from edenscm.mercurial import encoding, error, pycompat, util


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
        # This branch is unreachable on Python 2, because bytes == str
        # and we'll return in the next-earlier block in the elif
        # ladder. On Python 3, this helps us catch bugs before they
        # hurt someone.
        raise error.ProgrammingError(
            "Mercurial only does output with bytes on Python 3: %r" % obj
        )
    elif util.safehasattr(obj, "keys"):
        out = [
            '"%s": %s' % (encoding.jsonescape(k, paranoid=paranoid), dumps(v, paranoid))
            for k, v in sorted(obj.iteritems())
        ]
        return "{" + ", ".join(out) + "}"
    elif util.safehasattr(obj, "__iter__"):
        out = [dumps(i, paranoid) for i in obj]
        return "[" + ", ".join(out) + "]"
    else:
        raise TypeError("cannot encode type %s" % obj.__class__.__name__)


def _rapply(f, xs):
    if xs is None:
        # assume None means non-value of optional data
        return xs
    if isinstance(xs, (list, set, tuple)):
        return type(xs)(_rapply(f, x) for x in xs)
    if isinstance(xs, dict):
        return type(xs)((_rapply(f, k), _rapply(f, v)) for k, v in xs.items())
    return f(xs)


def loads(string):
    """Like stdlib json.loads, but results are bytes instead of unicode

    Warning: this does not round-trip with "dumps". "dumps" supports non-utf8
    binary content that is unsupported by this function.
    """
    # XXX: This should round-trip with "dumps". But it might be non-trivial to
    # do so.
    return _rapply(lambda s: bytes(s.encode("utf-8")), _sysjson.loads(string))
