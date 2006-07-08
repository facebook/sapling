# lock.py - simple locking scheme for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import *
demandload(globals(), 'errno os socket time util')

class LockException(IOError):
    def __init__(self, errno, strerror, filename, desc):
        IOError.__init__(self, errno, strerror, filename)
        self.desc = desc

class LockHeld(LockException):
    def __init__(self, errno, filename, desc, locker):
        LockException.__init__(self, errno, 'Lock held', filename, desc)
        self.locker = locker

class LockUnavailable(LockException):
    pass

class lock(object):
    # lock is symlink on platforms that support it, file on others.

    # symlink is used because create of directory entry and contents
    # are atomic even over nfs.

    # old-style lock: symlink to pid
    # new-style lock: symlink to hostname:pid

    def __init__(self, file, timeout=-1, releasefn=None, desc=None):
        self.f = file
        self.held = 0
        self.timeout = timeout
        self.releasefn = releasefn
        self.id = None
        self.host = None
        self.pid = None
        self.desc = desc
        self.lock()

    def __del__(self):
        self.release()

    def lock(self):
        timeout = self.timeout
        while 1:
            try:
                self.trylock()
                return 1
            except LockHeld, inst:
                if timeout != 0:
                    time.sleep(1)
                    if timeout > 0:
                        timeout -= 1
                    continue
                raise LockHeld(errno.ETIMEDOUT, inst.filename, self.desc,
                               inst.locker)

    def trylock(self):
        if self.id is None:
            self.host = socket.gethostname()
            self.pid = os.getpid()
            self.id = '%s:%s' % (self.host, self.pid)
        while not self.held:
            try:
                util.makelock(self.id, self.f)
                self.held = 1
            except (OSError, IOError), why:
                if why.errno == errno.EEXIST:
                    locker = self.testlock()
                    if locker:
                        raise LockHeld(errno.EAGAIN, self.f, self.desc,
                                       locker)
                else:
                    raise LockUnavailable(why.errno, why.strerror,
                                          why.filename, self.desc)

    def testlock(self):
        '''return id of locker if lock is valid, else None.'''
        # if old-style lock, we cannot tell what machine locker is on.
        # with new-style lock, if locker is on this machine, we can
        # see if locker is alive.  if locker is on this machine but
        # not alive, we can safely break lock.
        locker = util.readlock(self.f)
        try:
            host, pid = locker.split(":", 1)
        except ValueError:
            return locker
        if host != self.host:
            return locker
        try:
            pid = int(pid)
        except:
            return locker
        if util.testpid(pid):
            return locker
        # if locker dead, break lock.  must do this with another lock
        # held, or can race and break valid lock.
        try:
            l = lock(self.f + '.break')
            l.trylock()
            os.unlink(self.f)
            l.release()
        except (LockHeld, LockUnavailable):
            return locker

    def release(self):
        if self.held:
            self.held = 0
            if self.releasefn:
                self.releasefn()
            try:
                os.unlink(self.f)
            except: pass

