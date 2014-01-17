#!/usr/bin/env python

import os, sys, time, errno, signal

if os.name =='nt':
    import ctypes

    def _check(ret, expectederr=None):
        if ret == 0:
            winerrno = ctypes.GetLastError()
            if winerrno == expectederr:
                return True
            raise ctypes.WinError(winerrno)

    def kill(pid, logfn, tryhard=True):
        logfn('# Killing daemon process %d' % pid)
        PROCESS_TERMINATE = 1
        handle = ctypes.windll.kernel32.OpenProcess(
                PROCESS_TERMINATE, False, pid)
        if handle == 0:
            # TODO: call _check(0, expected) to check if "process not found"
            return # process not found, already finished
        try:
            _check(ctypes.windll.kernel32.TerminateProcess(handle, -1), 5)
            #      windows error 5 when process does not exist or no access TODO
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
        except OSError, err:
            if err.errno != errno.ESRCH:
                raise

def killdaemons(pidfile, tryhard=True, remove=False, logfn=None):
    if not logfn:
        logfn = lambda s: s
    # Kill off any leftover daemon processes
    try:
        fp = open(pidfile)
        for line in fp:
            try:
                pid = int(line)
            except ValueError:
                continue
            kill(pid, logfn, tryhard)
        fp.close()
        if remove:
            os.unlink(pidfile)
    except IOError:
        pass

if __name__ == '__main__':
    path, = sys.argv[1:]
    killdaemons(path)

