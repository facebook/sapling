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

_rccomponents = None

def rccomponents():
    '''return an ordered [(type, obj)] about where to load configs.

    respect $HGRCPATH. if $HGRCPATH is empty, only .hg/hgrc of current repo is
    used. if $HGRCPATH is not set, the platform default will be used.

    if a directory is provided, *.rc files under it will be used.

    type could be either 'path' or 'items', if type is 'path', obj is a string,
    and is the config file path. if type is 'items', obj is a list of (section,
    name, value, source) that should fill the config directly.
    '''
    global _rccomponents
    if _rccomponents is None:
        if 'HGRCPATH' in encoding.environ:
            _rccomponents = []
            for p in encoding.environ['HGRCPATH'].split(pycompat.ospathsep):
                if not p:
                    continue
                _rccomponents.extend(('path', p) for p in _expandrcpath(p))
        else:
            paths = defaultrcpath() + systemrcpath() + userrcpath()
            _rccomponents = [('path', os.path.normpath(p)) for p in paths]
    return _rccomponents
