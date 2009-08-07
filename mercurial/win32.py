# win32.py - utility functions that use win32 API
#
# Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

"""Utility functions that use win32 API.

Mark Hammond's win32all package allows better functionality on
Windows. This module overrides definitions in util.py. If not
available, import of this module will fail, and generic code will be
used.
"""

import win32api

import errno, os, sys, pywintypes, win32con, win32file, win32process
import winerror
import osutil, encoding
from win32com.shell import shell, shellcon

def os_link(src, dst):
    try:
        win32file.CreateHardLink(dst, src)
        # CreateHardLink sometimes succeeds on mapped drives but
        # following nlinks() returns 1. Check it now and bail out.
        if nlinks(src) < 2:
            try:
                win32file.DeleteFile(dst)
            except:
                pass
            # Fake hardlinking error
            raise OSError(errno.EINVAL, 'Hardlinking not supported')
    except pywintypes.error, details:
        raise OSError(errno.EINVAL, 'target implements hardlinks improperly')
    except NotImplementedError: # Another fake error win Win98
        raise OSError(errno.EINVAL, 'Hardlinking not supported')

def nlinks(pathname):
    """Return number of hardlinks for the given file."""
    try:
        fh = win32file.CreateFile(pathname,
            win32file.GENERIC_READ, win32file.FILE_SHARE_READ,
            None, win32file.OPEN_EXISTING, 0, None)
        res = win32file.GetFileInformationByHandle(fh)
        fh.Close()
        return res[7]
    except pywintypes.error:
        return os.lstat(pathname).st_nlink

def testpid(pid):
    '''return True if pid is still running or unable to
    determine, False otherwise'''
    try:
        handle = win32api.OpenProcess(
            win32con.PROCESS_QUERY_INFORMATION, False, pid)
        if handle:
            status = win32process.GetExitCodeProcess(handle)
            return status == win32con.STILL_ACTIVE
    except pywintypes.error, details:
        return details[0] != winerror.ERROR_INVALID_PARAMETER
    return True

def lookup_reg(key, valname=None, scope=None):
    ''' Look up a key/value name in the Windows registry.

    valname: value name. If unspecified, the default value for the key
    is used.
    scope: optionally specify scope for registry lookup, this can be
    a sequence of scopes to look up in order. Default (CURRENT_USER,
    LOCAL_MACHINE).
    '''
    try:
        from _winreg import HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, \
            QueryValueEx, OpenKey
    except ImportError:
        return None

    if scope is None:
        scope = (HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE)
    elif not isinstance(scope, (list, tuple)):
        scope = (scope,)
    for s in scope:
        try:
            val = QueryValueEx(OpenKey(s, key), valname)[0]
            # never let a Unicode string escape into the wild
            return encoding.tolocal(val.encode('UTF-8'))
        except EnvironmentError:
            pass

def system_rcpath_win32():
    '''return default os-specific hgrc search path'''
    proc = win32api.GetCurrentProcess()
    try:
        # This will fail on windows < NT
        filename = win32process.GetModuleFileNameEx(proc, 0)
    except:
        filename = win32api.GetModuleFileName(0)
    # Use mercurial.ini found in directory with hg.exe
    progrc = os.path.join(os.path.dirname(filename), 'mercurial.ini')
    if os.path.isfile(progrc):
        return [progrc]
    # else look for a system rcpath in the registry
    try:
        value = win32api.RegQueryValue(
                win32con.HKEY_LOCAL_MACHINE, 'SOFTWARE\\Mercurial')
        rcpath = []
        for p in value.split(os.pathsep):
            if p.lower().endswith('mercurial.ini'):
                rcpath.append(p)
            elif os.path.isdir(p):
                for f, kind in osutil.listdir(p):
                    if f.endswith('.rc'):
                        rcpath.append(os.path.join(p, f))
        return rcpath
    except pywintypes.error:
        return []

def user_rcpath_win32():
    '''return os-specific hgrc search path to the user dir'''
    userdir = os.path.expanduser('~')
    if sys.getwindowsversion()[3] != 2 and userdir == '~':
        # We are on win < nt: fetch the APPDATA directory location and use
        # the parent directory as the user home dir.
        appdir = shell.SHGetPathFromIDList(
            shell.SHGetSpecialFolderLocation(0, shellcon.CSIDL_APPDATA))
        userdir = os.path.dirname(appdir)
    return [os.path.join(userdir, 'mercurial.ini'),
            os.path.join(userdir, '.hgrc')]

def getuser():
    '''return name of current user'''
    return win32api.GetUserName()

def set_signal_handler_win32():
    """Register a termination handler for console events including
    CTRL+C. python signal handlers do not work well with socket
    operations.
    """
    def handler(event):
        win32process.ExitProcess(1)
    win32api.SetConsoleCtrlHandler(handler)

