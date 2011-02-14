# win32.py - utility functions that use win32 API
#
# Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import encoding
import ctypes, errno, os, struct, subprocess

_kernel32 = ctypes.windll.kernel32

_BOOL = ctypes.c_long
_WORD = ctypes.c_ushort
_DWORD = ctypes.c_ulong
_LPCSTR = _LPSTR = ctypes.c_char_p
_HANDLE = ctypes.c_void_p
_HWND = _HANDLE

_INVALID_HANDLE_VALUE = -1

# GetLastError
_ERROR_SUCCESS = 0
_ERROR_INVALID_PARAMETER = 87
_ERROR_INSUFFICIENT_BUFFER = 122

# WPARAM is defined as UINT_PTR (unsigned type)
# LPARAM is defined as LONG_PTR (signed type)
if ctypes.sizeof(ctypes.c_long) == ctypes.sizeof(ctypes.c_void_p):
    _WPARAM = ctypes.c_ulong
    _LPARAM = ctypes.c_long
elif ctypes.sizeof(ctypes.c_longlong) == ctypes.sizeof(ctypes.c_void_p):
    _WPARAM = ctypes.c_ulonglong
    _LPARAM = ctypes.c_longlong

class _FILETIME(ctypes.Structure):
    _fields_ = [('dwLowDateTime', _DWORD),
                ('dwHighDateTime', _DWORD)]

class _BY_HANDLE_FILE_INFORMATION(ctypes.Structure):
    _fields_ = [('dwFileAttributes', _DWORD),
                ('ftCreationTime', _FILETIME),
                ('ftLastAccessTime', _FILETIME),
                ('ftLastWriteTime', _FILETIME),
                ('dwVolumeSerialNumber', _DWORD),
                ('nFileSizeHigh', _DWORD),
                ('nFileSizeLow', _DWORD),
                ('nNumberOfLinks', _DWORD),
                ('nFileIndexHigh', _DWORD),
                ('nFileIndexLow', _DWORD)]

# CreateFile 
_FILE_SHARE_READ = 0x00000001
_FILE_SHARE_WRITE = 0x00000002
_FILE_SHARE_DELETE = 0x00000004

_OPEN_EXISTING = 3

# Process Security and Access Rights
_PROCESS_QUERY_INFORMATION = 0x0400

# GetExitCodeProcess
_STILL_ACTIVE = 259

# registry
_HKEY_CURRENT_USER = 0x80000001L
_HKEY_LOCAL_MACHINE = 0x80000002L
_KEY_READ = 0x20019
_REG_SZ = 1
_REG_DWORD = 4

class _STARTUPINFO(ctypes.Structure):
    _fields_ = [('cb', _DWORD),
                ('lpReserved', _LPSTR),
                ('lpDesktop', _LPSTR),
                ('lpTitle', _LPSTR),
                ('dwX', _DWORD),
                ('dwY', _DWORD),
                ('dwXSize', _DWORD),
                ('dwYSize', _DWORD),
                ('dwXCountChars', _DWORD),
                ('dwYCountChars', _DWORD),
                ('dwFillAttribute', _DWORD),
                ('dwFlags', _DWORD),
                ('wShowWindow', _WORD),
                ('cbReserved2', _WORD),
                ('lpReserved2', ctypes.c_char_p),
                ('hStdInput', _HANDLE),
                ('hStdOutput', _HANDLE),
                ('hStdError', _HANDLE)]

class _PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [('hProcess', _HANDLE),
                ('hThread', _HANDLE),
                ('dwProcessId', _DWORD),
                ('dwThreadId', _DWORD)]

_DETACHED_PROCESS = 0x00000008
_STARTF_USESHOWWINDOW = 0x00000001
_SW_HIDE = 0

class _COORD(ctypes.Structure):
    _fields_ = [('X', ctypes.c_short),
                ('Y', ctypes.c_short)]

class _SMALL_RECT(ctypes.Structure):
    _fields_ = [('Left', ctypes.c_short),
                ('Top', ctypes.c_short),
                ('Right', ctypes.c_short),
                ('Bottom', ctypes.c_short)]

class _CONSOLE_SCREEN_BUFFER_INFO(ctypes.Structure):
    _fields_ = [('dwSize', _COORD),
                ('dwCursorPosition', _COORD),
                ('wAttributes', _WORD),
                ('srWindow', _SMALL_RECT),
                ('dwMaximumWindowSize', _COORD)]

_STD_ERROR_HANDLE = 0xfffffff4L # (DWORD)-12

def _raiseoserror(name):
    err = ctypes.WinError()
    raise OSError(err.errno, '%s: %s' % (name, err.strerror))

def _getfileinfo(name):
    fh = _kernel32.CreateFileA(name, 0,
            _FILE_SHARE_READ | _FILE_SHARE_WRITE | _FILE_SHARE_DELETE,
            None, _OPEN_EXISTING, 0, None)
    if fh == _INVALID_HANDLE_VALUE:
        _raiseoserror(name)
    try:
        fi = _BY_HANDLE_FILE_INFORMATION()
        if not _kernel32.GetFileInformationByHandle(fh, ctypes.byref(fi)):
            _raiseoserror(name)
        return fi
    finally:
        _kernel32.CloseHandle(fh)

def os_link(src, dst):
    if not _kernel32.CreateHardLinkA(dst, src, None):
        _raiseoserror(src)

def nlinks(name):
    '''return number of hardlinks for the given file'''
    return _getfileinfo(name).nNumberOfLinks

def samefile(fpath1, fpath2):
    '''Returns whether fpath1 and fpath2 refer to the same file. This is only
    guaranteed to work for files, not directories.'''
    res1 = _getfileinfo(fpath1)
    res2 = _getfileinfo(fpath2)
    return (res1.dwVolumeSerialNumber == res2.dwVolumeSerialNumber
        and res1.nFileIndexHigh == res2.nFileIndexHigh
        and res1.nFileIndexLow == res2.nFileIndexLow)

def samedevice(fpath1, fpath2):
    '''Returns whether fpath1 and fpath2 are on the same device. This is only
    guaranteed to work for files, not directories.'''
    res1 = _getfileinfo(fpath1)
    res2 = _getfileinfo(fpath2)
    return res1.dwVolumeSerialNumber == res2.dwVolumeSerialNumber

def testpid(pid):
    '''return True if pid is still running or unable to
    determine, False otherwise'''
    h = _kernel32.OpenProcess(_PROCESS_QUERY_INFORMATION, False, pid)
    if h:
        try:
            status = _DWORD()
            if _kernel32.GetExitCodeProcess(h, ctypes.byref(status)):
                return status.value == _STILL_ACTIVE
        finally:
            _kernel32.CloseHandle(h)
    return _kernel32.GetLastError() != _ERROR_INVALID_PARAMETER

def lookup_reg(key, valname=None, scope=None):
    ''' Look up a key/value name in the Windows registry.

    valname: value name. If unspecified, the default value for the key
    is used.
    scope: optionally specify scope for registry lookup, this can be
    a sequence of scopes to look up in order. Default (CURRENT_USER,
    LOCAL_MACHINE).
    '''
    adv = ctypes.windll.advapi32
    byref = ctypes.byref
    if scope is None:
        scope = (_HKEY_CURRENT_USER, _HKEY_LOCAL_MACHINE)
    elif not isinstance(scope, (list, tuple)):
        scope = (scope,)
    for s in scope:
        kh = _HANDLE()
        res = adv.RegOpenKeyExA(s, key, 0, _KEY_READ, ctypes.byref(kh))
        if res != _ERROR_SUCCESS:
            continue
        try:
            size = _DWORD(600)
            type = _DWORD()
            buf = ctypes.create_string_buffer(size.value + 1)
            res = adv.RegQueryValueExA(kh.value, valname, None,
                                       byref(type), buf, byref(size))
            if res != _ERROR_SUCCESS:
                continue
            if type.value == _REG_SZ:
                # never let a Unicode string escape into the wild
                return encoding.tolocal(buf.value.encode('UTF-8'))
            elif type.value == _REG_DWORD:
                fmt = '<L'
                s = ctypes.string_at(byref(buf), struct.calcsize(fmt))
                return struct.unpack(fmt, s)[0]
        finally:
            adv.RegCloseKey(kh.value)

def executable_path():
    '''return full path of hg.exe'''
    size = 600
    buf = ctypes.create_string_buffer(size + 1)
    len = _kernel32.GetModuleFileNameA(None, ctypes.byref(buf), size)
    if len == 0:
        raise ctypes.WinError()
    elif len == size:
        raise ctypes.WinError(_ERROR_INSUFFICIENT_BUFFER)
    return buf.value

def getuser():
    '''return name of current user'''
    adv = ctypes.windll.advapi32
    size = _DWORD(300)
    buf = ctypes.create_string_buffer(size.value + 1)
    if not adv.GetUserNameA(ctypes.byref(buf), ctypes.byref(size)):
        raise ctypes.WinError()
    return buf.value

_SIGNAL_HANDLER = ctypes.WINFUNCTYPE(_BOOL, _DWORD)
_signal_handler = []

def set_signal_handler():
    '''Register a termination handler for console events including
    CTRL+C. python signal handlers do not work well with socket
    operations.
    '''
    def handler(event):
        _kernel32.ExitProcess(1)

    if _signal_handler:
        return # already registered
    h = _SIGNAL_HANDLER(handler)
    _signal_handler.append(h) # needed to prevent garbage collection
    if not _kernel32.SetConsoleCtrlHandler(h, True):
        raise ctypes.WinError()

_WNDENUMPROC = ctypes.WINFUNCTYPE(_BOOL, _HWND, _LPARAM)

def hidewindow():
    user32 = ctypes.windll.user32

    def callback(hwnd, pid):
        wpid = _DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
        if pid == wpid.value:
            user32.ShowWindow(hwnd, _SW_HIDE)
            return False # stop enumerating windows
        return True

    pid = _kernel32.GetCurrentProcessId()
    user32.EnumWindows(_WNDENUMPROC(callback), pid)

def termwidth():
    # cmd.exe does not handle CR like a unix console, the CR is
    # counted in the line length. On 80 columns consoles, if 80
    # characters are written, the following CR won't apply on the
    # current line but on the new one. Keep room for it.
    width = 79
    # Query stderr to avoid problems with redirections
    screenbuf = _kernel32.GetStdHandle(
                  _STD_ERROR_HANDLE) # don't close the handle returned
    if screenbuf is None or screenbuf == _INVALID_HANDLE_VALUE:
        return width
    csbi = _CONSOLE_SCREEN_BUFFER_INFO()
    if not _kernel32.GetConsoleScreenBufferInfo(
                        screenbuf, ctypes.byref(csbi)):
        return width
    width = csbi.srWindow.Right - csbi.srWindow.Left
    return width

def spawndetached(args):
    # No standard library function really spawns a fully detached
    # process under win32 because they allocate pipes or other objects
    # to handle standard streams communications. Passing these objects
    # to the child process requires handle inheritance to be enabled
    # which makes really detached processes impossible.
    si = _STARTUPINFO()
    si.cb = ctypes.sizeof(_STARTUPINFO)
    si.dwFlags = _STARTF_USESHOWWINDOW
    si.wShowWindow = _SW_HIDE

    pi = _PROCESS_INFORMATION()

    env = ''
    for k in os.environ:
        env += "%s=%s\0" % (k, os.environ[k])
    if not env:
        env = '\0'
    env += '\0'

    args = subprocess.list2cmdline(args)
    # Not running the command in shell mode makes python26 hang when
    # writing to hgweb output socket.
    comspec = os.environ.get("COMSPEC", "cmd.exe")
    args = comspec + " /c " + args

    res = _kernel32.CreateProcessA(
        None, args, None, None, False, _DETACHED_PROCESS,
        env, os.getcwd(), ctypes.byref(si), ctypes.byref(pi))
    if not res:
        raise ctypes.WinError()

    return pi.dwProcessId
