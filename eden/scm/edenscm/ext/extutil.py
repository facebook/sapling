# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# extutil.py - useful utility methods for extensions

from __future__ import absolute_import

import contextlib
import errno
import os
import time

from edenscm.mercurial import error, lock as lockmod, util, vfs as vfsmod


def replaceclass(container, classname):
    """Replace a class with another in a module, and interpose it into
    the hierarchies of all loaded subclasses. This function is
    intended for use as a decorator.

      import mymodule
      @replaceclass(mymodule, 'myclass')
      class mysubclass(mymodule.myclass):
          def foo(self):
              f = super(mysubclass, self).foo()
              return f + ' bar'

    Existing instances of the class being replaced will not have their
    __class__ modified, so call this function before creating any
    objects of the target type.
    """

    def wrap(cls):
        oldcls = getattr(container, classname)
        for subcls in oldcls.__subclasses__():
            if subcls is not cls:
                assert oldcls in subcls.__bases__
                newbases = [
                    oldbase for oldbase in subcls.__bases__ if oldbase != oldcls
                ]
                newbases.append(cls)
                subcls.__bases__ = tuple(newbases)
        setattr(container, classname, cls)
        return cls

    return wrap


@contextlib.contextmanager
def flock(lockpath, description, timeout=-1):
    """A flock based lock object. Currently it is always non-blocking.

    Note that since it is flock based, you can accidentally take it multiple
    times within one process and the first one to be released will release all
    of them. So the caller needs to be careful to not create more than one
    instance per lock.
    """

    # best effort lightweight lock
    try:
        import fcntl

        fcntl.flock
    except ImportError:
        # fallback to Mercurial lock
        vfs = vfsmod.vfs(os.path.dirname(lockpath))
        with lockmod.lock(vfs, os.path.basename(lockpath), timeout=timeout):
            yield
        return
    # make sure lock file exists
    util.makedirs(os.path.dirname(lockpath))
    with open(lockpath, "a"):
        pass
    lockfd = os.open(lockpath, os.O_RDWR, 0o664)
    start = time.time()
    while True:
        try:
            fcntl.flock(lockfd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except IOError as ex:
            if ex.errno == errno.EAGAIN:
                if timeout != -1 and time.time() - start > timeout:
                    raise error.LockHeld(errno.EAGAIN, lockpath, description, "")
                else:
                    time.sleep(0.05)
                    continue
            raise

    try:
        yield
    finally:
        fcntl.flock(lockfd, fcntl.LOCK_UN)
        os.close(lockfd)
