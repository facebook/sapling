import errno
import os
import signal
import socket
import sys
import time
try:
    import SocketServer
except ImportError:
    # Python 3
    import socketserver as SocketServer

# Make sure we use patchpython.py in this repo, unaffected by PYTHONPATH
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '../hgext3rd'))
import patchpython

assert patchpython # pass pyflakes "import but unused" check

def testnozombies():
    class reportpidhandler(SocketServer.StreamRequestHandler):
        def handle(self):
            self.wfile.write('%s' % (os.getpid(),))

    class server(SocketServer.ForkingMixIn, SocketServer.UnixStreamServer):
        pass

    socketpath = 'testsocket'
    pid = os.fork()

    if pid > 0:
        # client
        waittime = 0
        while not os.path.exists(socketpath):
            time.sleep(0.1)
            waittime += 0.1
            if waittime > 5:
                assert False, 'server timed out'
        try:
            pids = []
            for i in xrange(5):
                s = socket.socket(socket.AF_UNIX)
                s.connect(socketpath)
                buf = s.recv(1024)
                s.close()
                pids.append(int(buf))
            # give the server some time to do cleanup
            time.sleep(0.5)
            for p in pids:
                try:
                    os.kill(p, 0)
                    assert False, 'zombie process detected'
                except OSError as ex:
                    if ex.errno != errno.ESRCH:
                        raise
        finally:
            os.kill(pid, signal.SIGTERM) # stop server
            os.unlink(socketpath)
    else:
        # server
        s = server(socketpath, reportpidhandler)
        s.serve_forever(poll_interval=0.1)

testnozombies()
