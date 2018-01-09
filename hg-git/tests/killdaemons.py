#!/usr/bin/env python

from __future__ import absolute_import
import errno
import os
import signal
import sys
import time

if os.name =='nt':
    import ctypes

    _BOOL = ctypes.c_long
    _DWORD = ctypes.c_ulong
    _UINT = ctypes.c_uint
    _HANDLE = ctypes.c_void_p

    ctypes.windll.kernel32.CloseHandle.argtypes = [_HANDLE]
    ctypes.windll.kernel32.CloseHandle.restype = _BOOL

    ctypes.windll.kernel32.GetLastError.argtypes = []
    ctypes.windll.kernel32.GetLastError.restype = _DWORD

    ctypes.windll.kernel32.OpenProcess.argtypes = [_DWORD, _BOOL, _DWORD]
    ctypes.windll.kernel32.OpenProcess.restype = _HANDLE

    ctypes.windll.kernel32.TerminateProcess.argtypes = [_HANDLE, _UINT]
    ctypes.windll.kernel32.TerminateProcess.restype = _BOOL

    ctypes.windll.kernel32.WaitForSingleObject.argtypes = [_HANDLE, _DWORD]
    ctypes.windll.kernel32.WaitForSingleObject.restype = _DWORD

    def _check(ret, expectederr=None):
        if ret == 0:
            winerrno = ctypes.GetLastError()
            if winerrno == expectederr:
                return True
            raise ctypes.WinError(winerrno)

    def kill(pid, logfn, tryhard=True):
        logfn('# Killing daemon process %d' % pid)
        PROCESS_TERMINATE = 1
        PROCESS_QUERY_INFORMATION = 0x400
        SYNCHRONIZE = 0x00100000
        WAIT_OBJECT_0 = 0
        WAIT_TIMEOUT = 258
        WAIT_FAILED = _DWORD(0xFFFFFFFF).value
        handle = ctypes.windll.kernel32.OpenProcess(
                PROCESS_TERMINATE|SYNCHRONIZE|PROCESS_QUERY_INFORMATION,
                False, pid)
        if handle is None:
            _check(0, 87) # err 87 when process not found
            return # process not found, already finished
        try:
            r = ctypes.windll.kernel32.WaitForSingleObject(handle, 100)
            if r == WAIT_OBJECT_0:
                pass # terminated, but process handle still available
            elif r == WAIT_TIMEOUT:
                _check(ctypes.windll.kernel32.TerminateProcess(handle, -1))
            elif r == WAIT_FAILED:
                _check(0)  # err stored in GetLastError()

            # TODO?: forcefully kill when timeout
            #        and ?shorter waiting time? when tryhard==True
            r = ctypes.windll.kernel32.WaitForSingleObject(handle, 100)
                                                       # timeout = 100 ms
            if r == WAIT_OBJECT_0:
                pass # process is terminated
            elif r == WAIT_TIMEOUT:
                logfn('# Daemon process %d is stuck')
            elif r == WAIT_FAILED:
                _check(0)  # err stored in GetLastError()
        except: #re-raises
            ctypes.windll.kernel32.CloseHandle(handle) # no _check, keep error
            raise
        _check(ctypes.windll.kernel32.CloseHandle(handle))

else:
    def kill(pid, logfn, tryhard=True):
        try:
            os.kill(pid, 0)
            logfn('# Killing daemon process %d' % pid)
            os.kill(pid, signal.SIGTERM)
            if tryhard:
                for i in range(10):
                    time.sleep(0.05)
                    os.kill(pid, 0)
            else:
                time.sleep(0.1)
                os.kill(pid, 0)
            logfn('# Daemon process %d is stuck - really killing it' % pid)
            os.kill(pid, signal.SIGKILL)
        except OSError as err:
            if err.errno != errno.ESRCH:
                raise

def killdaemons(pidfile, tryhard=True, remove=False, logfn=None):
    if not logfn:
        logfn = lambda s: s
    # Kill off any leftover daemon processes
    try:
        pids = []
        with open(pidfile) as fp:
            for line in fp:
                try:
                    pid = int(line)
                    if pid <= 0:
                        raise ValueError
                except ValueError:
                    logfn('# Not killing daemon process %s - invalid pid'
                          % line.rstrip())
                    continue
                pids.append(pid)
        for pid in pids:
            kill(pid, logfn, tryhard)
        if remove:
            os.unlink(pidfile)
    except IOError:
        pass

if __name__ == '__main__':
    if len(sys.argv) > 1:
        path, = sys.argv[1:]
    else:
        path = os.environ["DAEMON_PIDS"]

    killdaemons(path)
