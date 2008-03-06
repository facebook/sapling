# util_win32.py - utility functions that use win32 API
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License, incorporated herein by reference.

# Mark Hammond's win32all package allows better functionality on
# Windows.  this module overrides definitions in util.py.  if not
# available, import of this module will fail, and generic code will be
# used.

import win32api

import errno, os, sys, pywintypes, win32con, win32file, win32process
import cStringIO, winerror
import osutil
from win32com.shell import shell,shellcon

class WinError:
    winerror_map = {
        winerror.ERROR_ACCESS_DENIED: errno.EACCES,
        winerror.ERROR_ACCOUNT_DISABLED: errno.EACCES,
        winerror.ERROR_ACCOUNT_RESTRICTION: errno.EACCES,
        winerror.ERROR_ALREADY_ASSIGNED: errno.EBUSY,
        winerror.ERROR_ALREADY_EXISTS: errno.EEXIST,
        winerror.ERROR_ARITHMETIC_OVERFLOW: errno.ERANGE,
        winerror.ERROR_BAD_COMMAND: errno.EIO,
        winerror.ERROR_BAD_DEVICE: errno.ENODEV,
        winerror.ERROR_BAD_DRIVER_LEVEL: errno.ENXIO,
        winerror.ERROR_BAD_EXE_FORMAT: errno.ENOEXEC,
        winerror.ERROR_BAD_FORMAT: errno.ENOEXEC,
        winerror.ERROR_BAD_LENGTH: errno.EINVAL,
        winerror.ERROR_BAD_PATHNAME: errno.ENOENT,
        winerror.ERROR_BAD_PIPE: errno.EPIPE,
        winerror.ERROR_BAD_UNIT: errno.ENODEV,
        winerror.ERROR_BAD_USERNAME: errno.EINVAL,
        winerror.ERROR_BROKEN_PIPE: errno.EPIPE,
        winerror.ERROR_BUFFER_OVERFLOW: errno.ENAMETOOLONG,
        winerror.ERROR_BUSY: errno.EBUSY,
        winerror.ERROR_BUSY_DRIVE: errno.EBUSY,
        winerror.ERROR_CALL_NOT_IMPLEMENTED: errno.ENOSYS,
        winerror.ERROR_CANNOT_MAKE: errno.EACCES,
        winerror.ERROR_CANTOPEN: errno.EIO,
        winerror.ERROR_CANTREAD: errno.EIO,
        winerror.ERROR_CANTWRITE: errno.EIO,
        winerror.ERROR_CRC: errno.EIO,
        winerror.ERROR_CURRENT_DIRECTORY: errno.EACCES,
        winerror.ERROR_DEVICE_IN_USE: errno.EBUSY,
        winerror.ERROR_DEV_NOT_EXIST: errno.ENODEV,
        winerror.ERROR_DIRECTORY: errno.EINVAL,
        winerror.ERROR_DIR_NOT_EMPTY: errno.ENOTEMPTY,
        winerror.ERROR_DISK_CHANGE: errno.EIO,
        winerror.ERROR_DISK_FULL: errno.ENOSPC,
        winerror.ERROR_DRIVE_LOCKED: errno.EBUSY,
        winerror.ERROR_ENVVAR_NOT_FOUND: errno.EINVAL,
        winerror.ERROR_EXE_MARKED_INVALID: errno.ENOEXEC,
        winerror.ERROR_FILENAME_EXCED_RANGE: errno.ENAMETOOLONG,
        winerror.ERROR_FILE_EXISTS: errno.EEXIST,
        winerror.ERROR_FILE_INVALID: errno.ENODEV,
        winerror.ERROR_FILE_NOT_FOUND: errno.ENOENT,
        winerror.ERROR_GEN_FAILURE: errno.EIO,
        winerror.ERROR_HANDLE_DISK_FULL: errno.ENOSPC,
        winerror.ERROR_INSUFFICIENT_BUFFER: errno.ENOMEM,
        winerror.ERROR_INVALID_ACCESS: errno.EACCES,
        winerror.ERROR_INVALID_ADDRESS: errno.EFAULT,
        winerror.ERROR_INVALID_BLOCK: errno.EFAULT,
        winerror.ERROR_INVALID_DATA: errno.EINVAL,
        winerror.ERROR_INVALID_DRIVE: errno.ENODEV,
        winerror.ERROR_INVALID_EXE_SIGNATURE: errno.ENOEXEC,
        winerror.ERROR_INVALID_FLAGS: errno.EINVAL,
        winerror.ERROR_INVALID_FUNCTION: errno.ENOSYS,
        winerror.ERROR_INVALID_HANDLE: errno.EBADF,
        winerror.ERROR_INVALID_LOGON_HOURS: errno.EACCES,
        winerror.ERROR_INVALID_NAME: errno.EINVAL,
        winerror.ERROR_INVALID_OWNER: errno.EINVAL,
        winerror.ERROR_INVALID_PARAMETER: errno.EINVAL,
        winerror.ERROR_INVALID_PASSWORD: errno.EPERM,
        winerror.ERROR_INVALID_PRIMARY_GROUP: errno.EINVAL,
        winerror.ERROR_INVALID_SIGNAL_NUMBER: errno.EINVAL,
        winerror.ERROR_INVALID_TARGET_HANDLE: errno.EIO,
        winerror.ERROR_INVALID_WORKSTATION: errno.EACCES,
        winerror.ERROR_IO_DEVICE: errno.EIO,
        winerror.ERROR_IO_INCOMPLETE: errno.EINTR,
        winerror.ERROR_LOCKED: errno.EBUSY,
        winerror.ERROR_LOCK_VIOLATION: errno.EACCES,
        winerror.ERROR_LOGON_FAILURE: errno.EACCES,
        winerror.ERROR_MAPPED_ALIGNMENT: errno.EINVAL,
        winerror.ERROR_META_EXPANSION_TOO_LONG: errno.E2BIG,
        winerror.ERROR_MORE_DATA: errno.EPIPE,
        winerror.ERROR_NEGATIVE_SEEK: errno.ESPIPE,
        winerror.ERROR_NOACCESS: errno.EFAULT,
        winerror.ERROR_NONE_MAPPED: errno.EINVAL,
        winerror.ERROR_NOT_ENOUGH_MEMORY: errno.ENOMEM,
        winerror.ERROR_NOT_READY: errno.EAGAIN,
        winerror.ERROR_NOT_SAME_DEVICE: errno.EXDEV,
        winerror.ERROR_NO_DATA: errno.EPIPE,
        winerror.ERROR_NO_MORE_SEARCH_HANDLES: errno.EIO,
        winerror.ERROR_NO_PROC_SLOTS: errno.EAGAIN,
        winerror.ERROR_NO_SUCH_PRIVILEGE: errno.EACCES,
        winerror.ERROR_OPEN_FAILED: errno.EIO,
        winerror.ERROR_OPEN_FILES: errno.EBUSY,
        winerror.ERROR_OPERATION_ABORTED: errno.EINTR,
        winerror.ERROR_OUTOFMEMORY: errno.ENOMEM,
        winerror.ERROR_PASSWORD_EXPIRED: errno.EACCES,
        winerror.ERROR_PATH_BUSY: errno.EBUSY,
        winerror.ERROR_PATH_NOT_FOUND: errno.ENOENT,
        winerror.ERROR_PIPE_BUSY: errno.EBUSY,
        winerror.ERROR_PIPE_CONNECTED: errno.EPIPE,
        winerror.ERROR_PIPE_LISTENING: errno.EPIPE,
        winerror.ERROR_PIPE_NOT_CONNECTED: errno.EPIPE,
        winerror.ERROR_PRIVILEGE_NOT_HELD: errno.EACCES,
        winerror.ERROR_READ_FAULT: errno.EIO,
        winerror.ERROR_SEEK: errno.EIO,
        winerror.ERROR_SEEK_ON_DEVICE: errno.ESPIPE,
        winerror.ERROR_SHARING_BUFFER_EXCEEDED: errno.ENFILE,
        winerror.ERROR_SHARING_VIOLATION: errno.EACCES,
        winerror.ERROR_STACK_OVERFLOW: errno.ENOMEM,
        winerror.ERROR_SWAPERROR: errno.ENOENT,
        winerror.ERROR_TOO_MANY_MODULES: errno.EMFILE,
        winerror.ERROR_TOO_MANY_OPEN_FILES: errno.EMFILE,
        winerror.ERROR_UNRECOGNIZED_MEDIA: errno.ENXIO,
        winerror.ERROR_UNRECOGNIZED_VOLUME: errno.ENODEV,
        winerror.ERROR_WAIT_NO_CHILDREN: errno.ECHILD,
        winerror.ERROR_WRITE_FAULT: errno.EIO,
        winerror.ERROR_WRITE_PROTECT: errno.EROFS,
        }

    def __init__(self, err):
        self.win_errno, self.win_function, self.win_strerror = err
        if self.win_strerror.endswith('.'):
            self.win_strerror = self.win_strerror[:-1]

class WinIOError(WinError, IOError):
    def __init__(self, err, filename=None):
        WinError.__init__(self, err)
        IOError.__init__(self, self.winerror_map.get(self.win_errno, 0),
                         self.win_strerror)
        self.filename = filename

class WinOSError(WinError, OSError):
    def __init__(self, err):
        WinError.__init__(self, err)
        OSError.__init__(self, self.winerror_map.get(self.win_errno, 0),
                         self.win_strerror)

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
            raise WinOSError((18, 'CreateHardLink', 'The system cannot '
                              'move the file to a different disk drive'))
    except pywintypes.error, details:
        raise WinOSError(details)

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

    def query_val(scope, key, valname):
        try:
            keyhandle = OpenKey(scope, key)
            return QueryValueEx(keyhandle, valname)[0]
        except EnvironmentError:
            return None

    if scope is None:
        scope = (HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE)
    elif not isinstance(scope, (list, tuple)):
        scope = (scope,)
    for s in scope:
        val = query_val(s, key, valname)
        if val is not None:
            return val

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
    if sys.getwindowsversion() != 2 and userdir == '~':
        # We are on win < nt: fetch the APPDATA directory location and use
        # the parent directory as the user home dir.
        appdir = shell.SHGetPathFromIDList(
            shell.SHGetSpecialFolderLocation(0, shellcon.CSIDL_APPDATA))
        userdir = os.path.dirname(appdir)
    return [os.path.join(userdir, 'mercurial.ini'),
            os.path.join(userdir, '.hgrc')]

class posixfile_nt(object):
    '''file object with posix-like semantics.  on windows, normal
    files can not be deleted or renamed if they are open. must open
    with win32file.FILE_SHARE_DELETE. this flag does not exist on
    windows < nt, so do not use this class there.'''

    # tried to use win32file._open_osfhandle to pass fd to os.fdopen,
    # but does not work at all. wrap win32 file api instead.

    def __init__(self, name, mode='rb'):
        self.closed = False
        self.name = name
        self.mode = mode
        access = 0
        if 'r' in mode or '+' in mode:
            access |= win32file.GENERIC_READ
        if 'w' in mode or 'a' in mode or '+' in mode:
            access |= win32file.GENERIC_WRITE
        if 'r' in mode:
            creation = win32file.OPEN_EXISTING
        elif 'a' in mode:
            creation = win32file.OPEN_ALWAYS
        else:
            creation = win32file.CREATE_ALWAYS
        try:
            self.handle = win32file.CreateFile(name,
                                               access,
                                               win32file.FILE_SHARE_READ |
                                               win32file.FILE_SHARE_WRITE |
                                               win32file.FILE_SHARE_DELETE,
                                               None,
                                               creation,
                                               win32file.FILE_ATTRIBUTE_NORMAL,
                                               0)
        except pywintypes.error, err:
            raise WinIOError(err, name)

    def __iter__(self):
        for line in self.read().splitlines(True):
            yield line

    def read(self, count=-1):
        try:
            cs = cStringIO.StringIO()
            while count:
                wincount = int(count)
                if wincount == -1:
                    wincount = 1048576
                val, data = win32file.ReadFile(self.handle, wincount)
                if not data: break
                cs.write(data)
                if count != -1:
                    count -= len(data)
            return cs.getvalue()
        except pywintypes.error, err:
            raise WinIOError(err)

    def write(self, data):
        try:
            if 'a' in self.mode:
                win32file.SetFilePointer(self.handle, 0, win32file.FILE_END)
            nwrit = 0
            while nwrit < len(data):
                val, nwrit = win32file.WriteFile(self.handle, data)
                data = data[nwrit:]
        except pywintypes.error, err:
            raise WinIOError(err)

    def writelines(self, sequence):
        for s in sequence:
            self.write(s)

    def seek(self, pos, whence=0):
        try:
            win32file.SetFilePointer(self.handle, int(pos), whence)
        except pywintypes.error, err:
            raise WinIOError(err)

    def tell(self):
        try:
            return win32file.SetFilePointer(self.handle, 0,
                                            win32file.FILE_CURRENT)
        except pywintypes.error, err:
            raise WinIOError(err)

    def close(self):
        if not self.closed:
            self.handle = None
            self.closed = True

    def flush(self):
        # we have no application-level buffering
        pass

    def truncate(self, pos=0):
        try:
            win32file.SetFilePointer(self.handle, int(pos),
                                     win32file.FILE_BEGIN)
            win32file.SetEndOfFile(self.handle)
        except pywintypes.error, err:
            raise WinIOError(err)

getuser_fallback = win32api.GetUserName

def set_signal_handler_win32():
    """Register a termination handler for console events including
    CTRL+C. python signal handlers do not work well with socket
    operations.
    """
    def handler(event):
        win32process.ExitProcess(1)
    win32api.SetConsoleCtrlHandler(handler)
