# lock.py - simple locking scheme for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import errno, os, socket, time, util

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

    _host = None

    def __init__(self, file, timeout=-1, releasefn=None, desc=None):
        self.f = file
        self.held = 0
        self.timeout = timeout
        self.releasefn = releasefn
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
        if lock._host is None:
            lock._host = socket.gethostname()
        lockname = '%s:%s' % (lock._host, os.getpid())
        while not self.held:
            try:
                util.makelock(lockname, self.f)
                self.held = 1
            except (OSError, IOError), why:
                if why.errno == errno.EEXIST:
                    locker = self.testlock()
                    if locker is not None:
                        raise LockHeld(errno.EAGAIN, self.f, self.desc,
                                       locker)
                else:
                    raise LockUnavailable(why.errno, why.strerror,
                                          why.filename, self.desc)

    def testlock(self):
        """return id of locker if lock is valid, else None.

        If old-style lock, we cannot tell what machine locker is on.
        with new-style lock, if locker is on this machine, we can
        see if locker is alive.  If locker is on this machine but
        not alive, we can safely break lock.

        The lock file is only deleted when None is returned.

        """
        locker = util.readlock(self.f)
        try:
            host, pid = locker.split(":", 1)
        except ValueError:
            return locker
        if host != lock._host:
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

