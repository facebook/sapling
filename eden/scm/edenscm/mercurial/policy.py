# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# policy.py - module policy logic for Mercurial.
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import sys


def _importfrom(modname):
    # from .edenscmnative import <modname> (where . is looked through this module)
    fakelocals = {}
    pkg = __import__("edenscmnative", globals(), fakelocals, [modname], level=0)
    try:
        fakelocals[modname] = mod = getattr(pkg, modname)
    except AttributeError:
        raise ImportError(r"cannot import name %s" % modname)
    # force import; fakelocals[modname] may be replaced with the real module
    getattr(mod, r"__doc__", None)
    return fakelocals[modname]


# keep in sync with "version" in C modules
_cextversions = {
    r"base85": 1,
    r"bdiff": 1,
    r"diffhelpers": 1,
    r"mpatch": 1,
    r"osutil": 2,
    r"parsers": 5,
}

# map import request to other package or module
_modredirects = {r"charencode": r"parsers"}


def _checkmod(modname, mod):
    expected = _cextversions.get(modname)
    actual = getattr(mod, r"version", None)
    if actual != expected:
        raise ImportError(
            r"cannot import module %s.%s "
            r"(expected version: %d, actual: %r)" % (modname, expected, actual)
        )


def importmod(modname):
    """Import module"""
    mn = _modredirects.get(modname, modname)
    mod = _importfrom(mn)
    _checkmod(mn, mod)
    return mod
