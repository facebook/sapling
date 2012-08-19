#!/usr/bin/env python

import os, time, errno, signal

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
        fp.close()
        if remove:
            os.unlink(pidfile)
    except IOError:
        pass

if __name__ == '__main__':
    killdaemons(os.environ['DAEMON_PIDS'])

