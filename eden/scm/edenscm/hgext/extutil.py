# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# extutil.py - useful utility methods for extensions

from __future__ import absolute_import

import contextlib
import errno
import os
import subprocess
import time

from edenscm.mercurial import error, lock as lockmod, pycompat, util, vfs as vfsmod


if pycompat.iswindows:
    CREATE_NO_WINDOW = 0x08000000
    # pyre-fixme[16]: Module `subprocess` has no attribute `CREATE_NEW_PROCESS_GROUP`.
    _creationflags = CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP

    def runbgcommand(script, env, shell=False, stdout=None, stderr=None):
        """Spawn a command without waiting for it to finish."""
        # According to the Python standard library, we can't use close_fds
        # *and* redirect std*. I'm not sure that we need to because the
        # detached process has no console connection.
        if stdout is not None or stderr is not None:
            raise error.ProgrammingError(
                "runbgcommand on Windows does not support stdout or stderr"
            )
        subprocess.Popen(
            script, shell=shell, env=env, close_fds=True, creationflags=_creationflags
        )


else:

    def runbgcommand(cmd, env, shell=False, stdout=None, stderr=None):
        """Spawn a command without waiting for it to finish."""
        parentpid = os.getpid()
        returncode = 255
        # Make sure os._exit is executed for all cases for the child process,
        # even if the user pressed Ctrl+C when Python is executing the "=",
        # aka. "STORE_FAST" of "pid = os.fork()", which has the bytecode:
        #
        #      LOAD_GLOBAL              0 (os)
        #      LOAD_ATTR                1 (fork)
        #      CALL_FUNCTION            0
        #      STORE_FAST               0 (pid)
        #
        # This means:
        # 1. "try, finally: os._exit" needs to be set up before executing
        #    "os.fork()".
        # 2. The "pid" variable cannot be used in the "finally" block.
        try:
            # double-fork to completely detach from the parent process
            # based on http://code.activestate.com/recipes/278731
            pid = os.fork()
            if pid:
                # Parent process
                (_pid, status) = os.waitpid(pid, 0)
                if os.WIFEXITED(status):
                    returncode = os.WEXITSTATUS(status)
                else:
                    returncode = -os.WTERMSIG(status)
                if returncode != 0:
                    # The child process's return code is 0 on success, an errno
                    # value on failure, or 255 if we don't have a valid errno
                    # value.
                    #
                    # (It would be slightly nicer to return the full exception info
                    # over a pipe as the subprocess module does.  For now it
                    # doesn't seem worth adding that complexity here, though.)
                    if returncode == 255:
                        returncode = errno.EINVAL
                    raise OSError(
                        returncode,
                        "error running %r: %s" % (cmd, os.strerror(returncode)),
                    )
                return

            try:
                # Start a new session
                os.setsid()

                stdin = open(os.devnull, "r")
                if stdout is None:
                    stdout = open(os.devnull, "w")
                if stderr is None:
                    stderr = open(os.devnull, "w")

                # connect stdin to devnull to make sure the subprocess can't
                # muck up that stream for mercurial.
                subprocess.Popen(
                    cmd,
                    shell=shell,
                    env=env,
                    close_fds=True,
                    stdin=stdin,
                    stdout=stdout,
                    stderr=stderr,
                )
                returncode = 0
            except EnvironmentError as ex:
                returncode = ex.errno & 0xFF
                if returncode == 0:
                    # This shouldn't happen, but just in case make sure the
                    # return code is never 0 here.
                    returncode = 255
            except Exception:
                returncode = 255
        finally:
            if os.getpid() != parentpid:
                # mission accomplished, this child needs to exit and not
                # continue the hg process here.
                os._exit(returncode)


def runshellcommand(script, env):
    """
    Run a shell command in the background.
    This spawns the command and returns before it completes.

    Prefer using runbgcommand() instead of this function.  This function should
    be discouraged in new code.  Running commands through a subshell requires
    you to be very careful about correctly escaping arguments, and you need to
    make sure your command works with both Windows and Unix shells.
    """
    runbgcommand(script, env=env, shell=True)


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
