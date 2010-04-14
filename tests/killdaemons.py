#!/usr/bin/env python

import os, time, errno, signal

# Kill off any leftover daemon processes
try:
    fp = open(os.environ['DAEMON_PIDS'])
    for line in fp:
        try:
            pid = int(line)
        except ValueError:
            continue
        try:
            os.kill(pid, 0)
            os.kill(pid, signal.SIGTERM)
            for i in range(10):
                time.sleep(0.05)
                os.kill(pid, 0)
            os.kill(pid, signal.SIGKILL)
        except OSError, err:
            if err.errno != errno.ESRCH:
                raise
    fp.close()
except IOError:
    pass
