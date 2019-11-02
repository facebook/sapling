# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import __builtin__
import os

from edenscm.mercurial import util


def lowerwrap(scope, funcname):
    f = getattr(scope, funcname)

    def wrap(fname, *args, **kwargs):
        d, base = os.path.split(fname)
        try:
            files = os.listdir(d or ".")
        except OSError:
            files = []
        if base in files:
            return f(fname, *args, **kwargs)
        for fn in files:
            if fn.lower() == base.lower():
                return f(os.path.join(d, fn), *args, **kwargs)
        return f(fname, *args, **kwargs)

    scope.__dict__[funcname] = wrap


def normcase(path):
    return path.lower()


os.path.normcase = normcase

for f in "file open".split():
    lowerwrap(__builtin__, f)

for f in "chmod chown open lstat stat remove unlink".split():
    lowerwrap(os, f)

for f in "exists lexists".split():
    lowerwrap(os.path, f)

lowerwrap(util, "posixfile")
