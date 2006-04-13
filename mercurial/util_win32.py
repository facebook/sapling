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

from demandload import *
from i18n import gettext as _
demandload(globals(), 'errno os pywintypes win32con win32file win32process')
demandload(globals(), 'winerror')

class WinError(OSError):
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
        winerror.ERROR_PATH_NOT_FOUND: errno.ENOTDIR,
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
        OSError.__init__(self, self.winerror_map.get(self.win_errno, 0),
                         self.win_strerror)

def os_link(src, dst):
    # NB will only succeed on NTFS
    try:
        win32file.CreateHardLink(dst, src)
    except pywintypes.error, details:
        raise WinError(details)

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
        return os.stat(pathname).st_nlink

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

def system_rcpath():
    '''return default os-specific hgrc search path'''
    proc = win32api.GetCurrentProcess()
    filename = win32process.GetModuleFileNameEx(proc, 0)
    return [os.path.join(os.path.dirname(filename), 'mercurial.ini')]
