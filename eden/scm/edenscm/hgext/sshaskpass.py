#!/usr/bin/env python
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""ssh-askpass implementation that works with chg

chg runs ssh at server side, and ssh does not have access to /dev/tty thus
unable to ask for password interactively when its output is being piped (ex.
during hg push or pull).

When ssh is unable to use /dev/tty, it will try to run SSH_ASKPASS if DISPLAY
is set, which is usually a GUI program. Here we set it to a special program
receiving fds from a simple unix socket server.

This file is both a mercurial extension to start that tty server and a
standalone ssh-askpass script.
"""

# Attention: Do NOT import anything inside mercurial here. This file also runs
# standalone without the mercurial environment, in which case it cannot import
# mercurial modules correctly.

import contextlib
import os
import signal
import socket
import sys
import tempfile

# pyre-fixme[21]: Could not find `reduction`.
from multiprocessing.reduction import recv_handle, send_handle


try:
    from edenscm.mercurial import encoding

    environ = encoding.environ
except ImportError:
    environ = getattr(os, "environ")


# backup tty fds. useful if we lose them later, like chg starting the pager
_ttyfds = []


@contextlib.contextmanager
def _silentexception(terminate=False):
    """silent common exceptions

    useful if we don't want to pollute the terminal
    exit if terminal is True
    """
    exitcode = 0
    try:
        yield
    except KeyboardInterrupt:
        exitcode = 1
    except Exception:
        exitcode = 2
    if terminate:
        os._exit(exitcode)


def _sockbind(sock, addr):
    """shim for util.bindunixsocket"""
    sock.bind(addr)


def _isttyserverneeded():
    # respect user's setting, SSH_ASKPASS is likely a gui program
    if "SSH_ASKPASS" in environ:
        return False

    # the tty server is not needed if /dev/tty can be opened
    try:
        with open("/dev/tty"):
            return False
    except Exception:
        pass

    # if no backup tty fds, and neither stdin nor stderr are tty, give up
    if not _ttyfds and not all(f.isatty() for f in [sys.stdin, sys.stderr]):
        return False

    # tty server is needed
    return True


def _startttyserver():
    """start a tty fd server

    the server will send tty read and write fds via unix socket

    listens at sockpath: $TMPDIR/ttysrv$UID/$PID
    returns (pid, sockpath)
    """
    sockpath = os.path.join(tempfile.mkdtemp("ttysrv"), str(os.getpid()))
    pipes = os.pipe()
    pid = os.fork()
    if pid:
        # parent, wait for the child to start listening
        os.close(pipes[1])
        os.read(pipes[0], 1)
        os.close(pipes[0])
        return pid, sockpath

    # child, starts the server
    ttyrfd, ttywfd = _ttyfds or [sys.stdin.fileno(), sys.stderr.fileno()]

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    getattr(util, "bindunixsocket", _sockbind)(sock, sockpath)
    sock.listen(1)

    # unblock parent
    os.close(pipes[0])
    os.write(pipes[1], " ")
    os.close(pipes[1])
    with _silentexception(terminate=True):
        while True:
            conn, addr = sock.accept()
            # 0: a dummy destination_pid, is ignored on posix systems
            send_handle(conn, ttyrfd, 0)
            send_handle(conn, ttywfd, 0)
            conn.close()


def _killprocess(pid):
    """kill and reap a child process"""
    os.kill(pid, signal.SIGTERM)
    try:
        os.waitpid(pid, 0)
    except KeyboardInterrupt:
        pass
    except Exception:
        pass


def _receivefds(sockpath):
    """get fds from the tty server listening at sockpath

    returns (readfd, writefd)
    """
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    # use chdir to handle long sockpath
    os.chdir(os.path.dirname(sockpath) or ".")
    sock.connect(os.path.basename(sockpath))
    rfd = recv_handle(sock)
    wfd = recv_handle(sock)
    return (rfd, wfd)


def _validaterepo(orig, self, sshcmd, args, remotecmd, sshenv=None):
    if not _isttyserverneeded():
        return orig(self, sshcmd, args, remotecmd, sshenv=sshenv)

    pid = sockpath = scriptpath = None
    with _silentexception(terminate=False):
        pid, sockpath = _startttyserver()
        scriptpath = sockpath + ".sh"
        with open(scriptpath, "w") as f:
            f.write(
                '#!/bin/bash\nexec %s %s "$@"'
                % (util.shellquote("python2"), util.shellquote(__file__))
            )
        os.chmod(scriptpath, 0o755)
        env = {
            # ssh will not use SSH_ASKPASS if DISPLAY is not set
            "DISPLAY": environ.get("DISPLAY", ""),
            "SSH_ASKPASS": util.shellquote(scriptpath),
            "TTYSOCK": util.shellquote(sockpath),
        }
        prefix = " ".join("%s=%s" % (k, v) for k, v in env.items())
        # modify sshcmd to include new environ
        sshcmd = "%s %s" % (prefix, sshcmd)

    try:
        return orig(self, sshcmd, args, remotecmd, sshenv=sshenv)
    finally:
        if pid:
            _killprocess(pid)
        for path in [scriptpath, sockpath]:
            if path and os.path.exists(path):
                util.unlinkpath(path, ignoremissing=True)


def _attachio(orig, self):
    orig(self)
    # backup read, write tty fds to _ttyfds
    if _ttyfds:
        return
    ui = self.ui
    if ui.fin.isatty() and ui.ferr.isatty():
        rfd = os.dup(ui.fin.fileno())
        wfd = os.dup(ui.ferr.fileno())
        _ttyfds[:] = [rfd, wfd]


def _patchchgserver():
    """patch chgserver so we can backup tty fds before they are replaced if
    chg starts the pager.
    """
    chgserver = None
    try:
        from edenscm.mercurial import chgserver
    except ImportError:
        try:
            chgserver = extensions.find("chgserver")
        except KeyError:
            pass
    server = getattr(chgserver, "chgcmdserver", None)
    if server and "attachio" in server.capabilities:
        orig = server.attachio
        server.capabilities["attachio"] = extensions.bind(_attachio, orig)


def uisetup(ui):
    # _validaterepo runs ssh and needs to be wrapped
    extensions.wrapfunction(sshpeer.sshpeer, "_validaterepo", _validaterepo)
    _patchchgserver()


def _setecho(ttyr, enableecho):
    import termios

    attrs = termios.tcgetattr(ttyr)
    if bool(enableecho) == bool(attrs[3] & termios.ECHO):
        return
    attrs[3] ^= termios.ECHO
    termios.tcsetattr(ttyr, termios.TCSANOW, attrs)


def _shoulddisableecho(prompt):
    # we don't have the "flag" information from openssh's
    # read_passphrase(const char *prompt, int flags).
    # guess from the prompt string.
    # do not match "Passcode or option"
    if "Passcode or option" in prompt:
        return False
    # match "password", "Password", "passphrase", "Passphrase".
    return prompt.find("ass") >= 0


def _sshaskpassmain(prompt):
    """the ssh-askpass client"""
    rfd, wfd = _receivefds(environ["TTYSOCK"])
    # cannot use util.fdopen here - the script runs outside "edenscm" context.
    r, w = os.fdopen(rfd, "r"), os.fdopen(wfd, "a")
    w.write("\033[31;1m==== AUTHENTICATING FOR SSH  ====\033[0m\n")
    w.write(prompt)
    w.flush()
    shouldecho = not _shoulddisableecho(prompt)
    _setecho(r, shouldecho)
    try:
        line = r.readline()
    finally:
        if not shouldecho:
            w.write("\n")
        _setecho(r, True)
    sys.stdout.write(line)
    sys.stdout.flush()
    w.write("\033[31;1m==== AUTHENTICATION COMPLETE ====\033[0m\n")


if __name__ == "__main__" and all(n in environ for n in ["SSH_ASKPASS", "TTYSOCK"]):
    # started by ssh as ssh-askpass
    with _silentexception(terminate=True):
        _sshaskpassmain(" ".join(sys.argv[1:]))
else:
    # imported as a mercurial extension
    from edenscm.mercurial import extensions, sshpeer, util
