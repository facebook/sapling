from __future__ import absolute_import

import os

from . import (
    encoding,
    pycompat,
    util,
    win32,
)

try:
    import _winreg as winreg
    winreg.CloseKey
except ImportError:
    import winreg

# MS-DOS 'more' is the only pager available by default on Windows.
fallbackpager = 'more'

def systemrcpath():
    '''return default os-specific hgrc search path'''
    rcpath = []
    filename = util.executablepath()
    # Use mercurial.ini found in directory with hg.exe
    progrc = os.path.join(os.path.dirname(filename), 'mercurial.ini')
    rcpath.append(progrc)
    # Use hgrc.d found in directory with hg.exe
    progrcd = os.path.join(os.path.dirname(filename), 'hgrc.d')
    if os.path.isdir(progrcd):
        for f, kind in util.listdir(progrcd):
            if f.endswith('.rc'):
                rcpath.append(os.path.join(progrcd, f))
    # else look for a system rcpath in the registry
    value = util.lookupreg('SOFTWARE\\Mercurial', None,
                           winreg.HKEY_LOCAL_MACHINE)
    if not isinstance(value, str) or not value:
        return rcpath
    value = util.localpath(value)
    for p in value.split(pycompat.ospathsep):
        if p.lower().endswith('mercurial.ini'):
            rcpath.append(p)
        elif os.path.isdir(p):
            for f, kind in util.listdir(p):
                if f.endswith('.rc'):
                    rcpath.append(os.path.join(p, f))
    return rcpath

def userrcpath():
    '''return os-specific hgrc search path to the user dir'''
    home = os.path.expanduser('~')
    path = [os.path.join(home, 'mercurial.ini'),
            os.path.join(home, '.hgrc')]
    userprofile = encoding.environ.get('USERPROFILE')
    if userprofile and userprofile != home:
        path.append(os.path.join(userprofile, 'mercurial.ini'))
        path.append(os.path.join(userprofile, '.hgrc'))
    return path

def termsize(ui):
    return win32.termsize()
