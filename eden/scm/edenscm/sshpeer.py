# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sshpeer.py - ssh repository proxy class for mercurial
#
# Copyright 2005, 2006 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import io
import os
import re
import subprocess
import threading
import weakref
from typing import Any, Optional, Tuple

from . import error, progress, stdiopeer, util
from .i18n import _
from .pycompat import decodeutf8


# Record of the bytes sent and received to SSH peers.  This records the
# cumulative total bytes sent to all peers for the life of the process.
_totalbytessent = 0
_totalbytesreceived = 0


def _serverquote(s: str) -> str:
    if not s:
        return s
    """quote a string for the remote shell ... which we assume is sh"""
    if re.match("[a-zA-Z0-9@%_+=:,./-]*$", s):
        return s
    return "'%s'" % s.replace("'", "'\\''")


def _writessherror(ui: "Any", s: bytes) -> None:
    if s and not ui.quiet:
        for l in s.splitlines():
            if l.startswith(b"ssh:"):
                prefix = ""
            else:
                prefix = _("remote: ")
            ui.write_err(prefix, decodeutf8(l, errors="replace"), "\n")


class countingpipe(object):
    """Wraps a pipe that count the number of bytes read/written to it"""

    def __init__(self, ui, pipe):
        self._ui = ui
        self._pipe = pipe
        self._totalbytes = 0

    def write(self, data):
        assert isinstance(data, bytes)
        self._totalbytes += len(data)
        self._ui.metrics.gauge("ssh_write_bytes", len(data))
        return self._pipe.write(data)

    def read(self, size: int) -> bytes:
        r = self._pipe.read(size)
        bufs = [r]
        # In Python 3 _pipe is a FileIO and is not guaranteed to return size
        # bytes. So let's loop until we get the bytes, or we get 0 bytes,
        # indicating the end of the pipe.
        if len(r) < size:
            totalread = len(r)
            while totalread < size and len(r) != 0:
                r = self._pipe.read(size - totalread)
                totalread += len(r)
                bufs.append(r)

        r = b"".join(bufs)

        self._totalbytes += len(r)
        self._ui.metrics.gauge("ssh_read_bytes", len(r))
        return r

    def readline(self):
        r = self._pipe.readline()
        self._totalbytes += len(r)
        self._ui.metrics.gauge("ssh_read_bytes", len(r))
        return r

    def close(self):
        return self._pipe.close()

    def flush(self):
        return self._pipe.flush()


class threadedstderr(object):
    def __init__(self, ui, stderr):
        self._ui = ui
        self._stderr = stderr
        self._thread = None

    def start(self) -> None:
        thread = threading.Thread(target=self.run)
        thread.daemon = True
        thread.start()
        self._thread = thread

    def run(self) -> None:
        while True:
            try:
                buf = self._stderr.readline()
            except (Exception, KeyboardInterrupt):
                # Not fatal. Treat it as if the stderr stream has ended.
                break
            if len(buf) == 0:
                break

            _writessherror(self._ui, buf)

        # Close the pipe. It's likely already closed on the other end.
        # Note: during readline(), close() will raise an IOError. So there is
        # no "close" method that can be used by the main thread.
        self._stderr.close()

    def join(self, timeout):
        if self._thread:
            self._thread.join(timeout)


_sshpeerweakrefs = []


def cleanupall() -> None:
    """Call _cleanup for all remaining sshpeers.

    sshpeer.__del__ -> _cleanup -> thread.join might cause deadlock
    if triggered by Python 3's PyFinalize -> GC. Calling this before exiting
    the main thread would prevent that deadlock.
    """
    for ref in _sshpeerweakrefs:
        peer = ref()
        if peer is not None:
            peer._cleanup()
    _sshpeerweakrefs[:] = []


class sshpeer(stdiopeer.stdiopeer):
    def __init__(self, ui, path, create=False, initial_config: Optional[str] = None):
        super(sshpeer, self).__init__(
            ui, path, create=create, initial_config=initial_config
        )
        self._pipee = None

        u = util.url(path, parsequery=False, parsefragment=False)
        if u.scheme != "ssh" or not u.host or u.path is None:
            self._abort(error.RepoError(_("couldn't parse location %s") % path))

        util.checksafessh(path)

        if u.passwd is not None:
            self._abort(error.RepoError(_("password in URL not supported")))

        self._user = u.user
        self._host = u.host
        self._port = u.port
        self._path = u.path or "."

        sshcmd = self.ui.config("ui", "ssh")
        remotecmd = self.ui.config("ui", "remotecmd")
        sshaddenv = dict(self.ui.configitems("sshenv"))
        sshenv = util.shellenviron(sshaddenv)

        args = util.sshargs(sshcmd, self._host, self._user, self._port)

        if create:
            cmd = "%s %s %s" % (
                sshcmd,
                args,
                util.shellquote(
                    "%s init %s" % (_serverquote(remotecmd), _serverquote(self._path))
                ),
            )
            ui.debug("running %s\n" % cmd)
            res = ui.system(cmd, blockedtag="sshpeer", environ=sshenv)
            if res != 0:
                self._abort(error.RepoError(_("could not create remote repo")))

        _sshpeerweakrefs.append(weakref.ref(self))
        with self.ui.timeblockedsection("sshsetup"), progress.suspend(), util.traced(
            "ssh_setup", cat="blocked"
        ):
            self._validaterepo(sshcmd, args, remotecmd, sshenv)

    def _validaterepo(self, sshcmd, args, remotecmd, sshenv=None):
        # cleanup up previous run
        self._cleanup()

        cmd = "%s %s %s" % (
            sshcmd,
            args,
            util.shellquote(
                "%s -R %s serve --stdio"
                % (_serverquote(remotecmd), _serverquote(self._path))
            ),
        )
        self.ui.debug("running %s\n" % cmd)

        # while self._subprocess isn't used, having it allows the subprocess to
        # to clean up correctly later
        if util.istest():
            # spwan 'hg serve' directly, avoid depending on /bin/sh or python
            # to run dummyssh
            sub = _popen4testhgserve(self._path, env=sshenv)
        else:
            sub = util.popen4(cmd, bufsize=0, env=sshenv)
        pipeo, pipei, pipee, self._subprocess = sub

        self._pipee = threadedstderr(self.ui, pipee)
        self._pipee.start()
        self._pipei = countingpipe(self.ui, pipei)
        self._pipeo = countingpipe(self.ui, pipeo)

        self.ui.metrics.gauge("ssh_connections")

        def badresponse(errortext):
            msg = _("no suitable response from remote @prog@")
            if errortext:
                msg += ": '%s'" % errortext
            hint = self.ui.config("ui", "ssherrorhint")
            self._abort(error.BadResponseError(msg, hint=hint))

        timer = None
        try:

            def timeout():
                self.ui.warn(
                    _("timed out establishing the ssh connection, killing ssh\n")
                )
                self._subprocess.kill()

            sshsetuptimeout = self.ui.configint("ui", "sshsetuptimeout")
            if sshsetuptimeout:
                timer = threading.Timer(sshsetuptimeout, timeout)
                timer.start()

            try:
                # skip any noise generated by remote shell
                self._callstream("hello")
                r = self._callstream("between", pairs=("%s-%s" % ("0" * 40, "0" * 40)))
            except IOError as ex:
                badresponse(str(ex))

            lines = ["", "dummy"]
            max_noise = 500
            while lines[-1] and max_noise:
                try:
                    l = decodeutf8(r.readline())
                    if lines[-1] == "1\n" and l == "\n":
                        break
                    if l:
                        self.ui.debug("remote: ", l)
                    lines.append(l)
                    max_noise -= 1
                except IOError as ex:
                    badresponse(str(ex))
            else:
                badresponse("".join(lines[2:]))
        finally:
            if timer:
                timer.cancel()

        for l in reversed(lines):
            if l.startswith("capabilities:"):
                self._caps.update(l[:-1].split(":")[1].split())
                break

    def _cleanup(self):
        global _totalbytessent, _totalbytesreceived
        if self._pipeo is None:
            return

        # Close the pipe connecting to the stdin of the remote ssh process.
        # This means if the remote process tries to read its stdin, it will get
        # an empty buffer that indicates EOF. The remote process should then
        # exit, which will close its stdout and stderr so the background stderr
        # reader thread will notice that it reaches EOF and becomes joinable.
        self._pipeo.close()

        _totalbytessent += self._pipeo._totalbytes

        # Clear the pipe to indicate this has already been cleaned up.
        self._pipeo = None

        # Wait for the stderr thread to complete reading all stderr text from
        # the remote ssh process (i.e. hitting EOF).
        #
        # This must be after pipeo.close(). Otherwise the remote process might
        # still wait for stdin and does not close its stderr.
        #
        # This is better before pipei.close(). Otherwise the remote process
        # might nondeterministically get EPIPE when writing to its stdout,
        # which can trigger different code paths nondeterministically that
        # might affect stderr. In other words, moving this after pipei.close()
        # can potentially increase test flakiness.
        if util.istest():
            # In the test environment, we control all remote processes. They
            # are expected to exit after getting EOF from stdin.  Wait
            # indefinitely to make sure all stderr messages are received.
            #
            # If this line hangs forever, that indicates a bug in the remote
            # process, not here.
            self._pipee.join(None)
        else:
            # In real world environment, remote processes might mis-behave.
            # Therefore be inpatient on waiting.
            self._pipee.join(1)

        # Close the pipe connected to the stdout of the remote process.
        # The remote end of the pipe is likely already closed since we waited
        # the pipee thread. If not, the remote process will get EPIPE or
        # SIGPIPE if it writes a bit more to its stdout.
        self._pipei.close()

        _totalbytesreceived += self._pipei._totalbytes
        self.ui.log(
            "sshbytes",
            "",
            sshbytessent=_totalbytessent,
            sshbytesreceived=_totalbytesreceived,
        )

    __del__ = _cleanup


def _pipe() -> Tuple[io.BufferedReader, io.BufferedWriter]:
    rfd, wfd = os.pipe()
    return os.fdopen(rfd, "rb"), os.fdopen(wfd, "wb")


def _popen4testhgserve(path, env=None, newlines: bool = False, bufsize: int = -1):
    """spawn 'hg serve' without depending on /bin/sh or cmd.exe or /usr/bin/env python or dummyssh"""
    assert util.istest()

    path = path.split("?", 1)[0]
    cmdargs = util.hgcmd() + ["-R", path, "serve", "--stdio"]
    testtmp = os.getenv("TESTTMP")

    def _isbashscript(file):
        if cmdargs[0].endswith(".sh"):
            return True

        with open(file, "rb") as file:
            start = file.read(12)
            return (b"#!/bin/bash" in start) or (b"#!/bin/sh" in start)
        return False

    # Append "defpath" (ex. /bin) to PATH.  Needed for buck test (the main hg
    # binary is a _bash_ script setting up a bunch of things instead of a
    # standalone single executable), combined with debugruntest (where PATH does
    # not include /bin to force external dependencies to be explicit).
    if _isbashscript(cmdargs[0]) and env:
        path = env.get("PATH")
        if path:
            env["PATH"] = os.pathsep.join([path, os.path.defpath])

    p = subprocess.Popen(
        cmdargs,
        cwd=testtmp,
        # shell=False avoids /bin/sh or cmd.exe
        shell=False,
        bufsize=bufsize,
        close_fds=util.closefds,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        universal_newlines=newlines,
        env=env,
    )

    # see D19872612
    def delayoutput(reader, writer):
        buf = io.BytesIO()
        while True:
            ch = reader.read(1)
            if not ch:
                break
            buf.write(ch)
        writer.write(buf.getvalue())
        writer.close()

    errread, errwrite = _pipe()
    t = threading.Thread(target=delayoutput, args=(p.stderr, errwrite), daemon=True)
    t.start()

    return p.stdin, p.stdout, errread, p


instance = sshpeer
