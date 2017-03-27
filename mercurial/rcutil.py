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

def defaultrcpath():
    '''return rc paths in default.d'''
    path = []
    defaultpath = os.path.join(util.datapath, 'default.d')
    if os.path.isdir(defaultpath):
        for f, kind in osutil.listdir(defaultpath):
            if f.endswith('.rc'):
                path.append(os.path.join(defaultpath, f))
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
                p = util.expandpath(p)
                if os.path.isdir(p):
                    for f, kind in osutil.listdir(p):
                        if f.endswith('.rc'):
                            _rcpath.append(os.path.join(p, f))
                else:
                    _rcpath.append(p)
        else:
            paths = defaultrcpath() + systemrcpath() + userrcpath()
            _rcpath = pycompat.maplist(os.path.normpath, paths)
    return _rcpath
