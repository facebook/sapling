# rcutil.py - utilities about config paths, special config sections etc.
#
#  Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from . import (
    encoding,
    osutil,
    pycompat,
    util,
)

if pycompat.osname == 'nt':
    from . import scmwindows as scmplatform
else:
    from . import scmposix as scmplatform

systemrcpath = scmplatform.systemrcpath
userrcpath = scmplatform.userrcpath

def _expandrcpath(path):
    '''path could be a file or a directory. return a list of file paths'''
    p = util.expandpath(path)
    if os.path.isdir(p):
        join = os.path.join
        return [join(p, f) for f, k in osutil.listdir(p) if f.endswith('.rc')]
    return [p]

def defaultrcpath():
    '''return rc paths in default.d'''
    path = []
    defaultpath = os.path.join(util.datapath, 'default.d')
    if os.path.isdir(defaultpath):
        path = _expandrcpath(defaultpath)
    return path

_rcpath = None

def rcpath():
    '''return hgrc search path. if env var HGRCPATH is set, use it.
    for each item in path, if directory, use files ending in .rc,
    else use item.
    make HGRCPATH empty to only look in .hg/hgrc of current repo.
    if no HGRCPATH, use default os-specific path.'''
    global _rcpath
    if _rcpath is None:
        if 'HGRCPATH' in encoding.environ:
            _rcpath = []
            for p in encoding.environ['HGRCPATH'].split(pycompat.ospathsep):
                if not p:
                    continue
                _rcpath.extend(_expandrcpath(p))
        else:
            paths = defaultrcpath() + systemrcpath() + userrcpath()
            _rcpath = pycompat.maplist(os.path.normpath, paths)
    return _rcpath
