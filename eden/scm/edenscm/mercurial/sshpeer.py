# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sshpeer.py - ssh repository proxy class for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re
import threading
from typing import Any

from . import error, progress, pycompat, util, wireproto
from .i18n import _


# Record of the bytes sent and received to SSH peers.  This records the
# cumulative total bytes sent to all peers for the life of the process.
_totalbytessent = 0
_totalbytesreceived = 0


def _serverquote(s):
    if not s:
        return s
    """quote a string for the remote shell ... which we assume is sh"""
    if re.match("[a-zA-Z0-9@%_+=:,./-]*$", s):
        return s
    return "'%s'" % s.replace("'", "'\\''")


def _writessherror(ui, s):
    # type: (Any, bytes) -> None
    if s and not ui.quiet:
        for l in s.splitlines():
            if l.startswith(b"ssh:"):
                prefix = b""
            else:
                prefix = _(b"remote: ")
            ui.write_err(prefix, l, b"\n")


class countingpipe(object):
    """Wraps a pipe that count the number of bytes read/written to it
    """

    def __init__(self, ui, pipe):
        self._ui = ui
        self._pipe = pipe
        self._totalbytes = 0

    def write(self, data):
        self._totalbytes += len(data)
        self._ui.metrics.gauge("ssh_write_bytes", len(data))
        return self._pipe.write(data)

    def read(self, size):
        r = self._pipe.read(size)
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


class threadedstderr(threading.Thread):
    def __init__(self, ui, stderr):
        self._ui = ui
        self._stderr = stderr
        self._stop = False
        threading.Thread.__init__(self)
        self.daemon = True

    def run(self):
        # type: () -> None
        while not self._stop:
            buf = self._stderr.readline()
            if len(buf) == 0:
                break

            _writessherror(self._ui, buf)

    def close(self):
        # type: () -> None
        self._stop = True


class sshpeer(wireproto.wirepeer):
    def __init__(self, ui, path, create=False):
        self._url = path
        self._ui = ui
        self._pipeo = self._pipei = self._pipee = None

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

        with self.ui.timeblockedsection("sshsetup"), progress.suspend():
            self._validaterepo(sshcmd, args, remotecmd, sshenv)

    # Begin of _basepeer interface.

    @util.propertycache
    def ui(self):
        return self._ui

    def url(self):
        return self._url

    def local(self):
        return None

    def peer(self):
        return self

    def canpush(self):
        return True

    def close(self):
        pass

    # End of _basepeer interface.

    # Begin of _basewirecommands interface.

    def capabilities(self):
        return self._caps

    # End of _basewirecommands interface.

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
        cmd = util.quotecommand(cmd)

        # while self._subprocess isn't used, having it allows the subprocess to
        # to clean up correctly later
        sub = util.popen4(cmd, bufsize=0, env=sshenv)
        self._pipeo, self._pipei, self._pipee, self._subprocess = sub

        self._pipee = threadedstderr(self.ui, self._pipee)
        self._pipee.start()
        self._pipei = countingpipe(self.ui, self._pipei)
        self._pipeo = countingpipe(self.ui, self._pipeo)

        self.ui.metrics.gauge("ssh_connections")

        def badresponse(errortext):
            msg = _("no suitable response from remote hg")
            if errortext:
                msg += ": '%s'" % errortext
            hint = self.ui.config("ui", "ssherrorhint")
            self._abort(error.RepoError(msg, hint=hint))

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
                l = r.readline()
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

        self._caps = set()
        for l in reversed(lines):
            if l.startswith("capabilities:"):
                self._caps.update(l[:-1].split(":")[1].split())
                break

    def _abort(self, exception):
        self._cleanup()
        raise exception

    def _cleanup(self):
        global _totalbytessent, _totalbytesreceived
        if self._pipeo is None:
            return
        self._pipeo.close()
        self._pipei.close()
        _totalbytessent += self._pipeo._totalbytes
        _totalbytesreceived += self._pipei._totalbytes
        self.ui.log(
            "sshbytes",
            "",
            sshbytessent=_totalbytessent,
            sshbytesreceived=_totalbytesreceived,
        )
        self._pipee.close()

        if util.istest():
            # Let's give the thread a bit of time to complete, in the case
            # where the pipe is somehow still open, the read call will block on it
            # forever. In this case, there isn't anything to read anyway, so
            # waiting more would just cause Mercurial to hang.
            self._pipee.join(1)

    __del__ = _cleanup

    def _submitbatch(self, req):
        rsp = self._callstream("batch", cmds=wireproto.encodebatchcmds(req))
        available = self._getamount()
        # TODO this response parsing is probably suboptimal for large
        # batches with large responses.
        toread = min(available, 1024)
        work = rsp.read(toread)
        available -= toread
        chunk = work
        while chunk:
            while ";" in work:
                one, work = work.split(";", 1)
                yield wireproto.unescapearg(one)
            toread = min(available, 1024)
            chunk = rsp.read(toread)
            available -= toread
            work += chunk
        yield wireproto.unescapearg(work)

    def _callstream(self, cmd, **args):
        args = pycompat.byteskwargs(args)
        self.ui.debug("sending %s command\n" % cmd)
        self._pipeo.write("%s\n" % cmd)
        _func, names = wireproto.commands[cmd]
        keys = names.split()
        wireargs = {}
        for k in keys:
            if k == "*":
                wireargs["*"] = args
                break
            else:
                wireargs[k] = args[k]
                del args[k]
        for k, v in sorted(wireargs.iteritems()):
            self._pipeo.write("%s %d\n" % (k, len(v)))
            if isinstance(v, dict):
                for dk, dv in v.iteritems():
                    self._pipeo.write("%s %d\n" % (dk, len(dv)))
                    self._pipeo.write(dv)
            else:
                self._pipeo.write(v)
        self._pipeo.flush()

        return self._pipei

    def _callcompressable(self, cmd, **args):
        return self._callstream(cmd, **args)

    def _call(self, cmd, **args):
        self._callstream(cmd, **args)
        return self._recv()

    def _callpush(self, cmd, fp, **args):
        r = self._call(cmd, **args)
        if r:
            return "", r
        for d in iter(lambda: fp.read(4096), ""):
            self._send(d)
        self._send("", flush=True)
        r = self._recv()
        if r:
            return "", r
        return self._recv(), ""

    def _calltwowaystream(self, cmd, fp, **args):
        r = self._call(cmd, **args)
        if r:
            # XXX needs to be made better
            raise error.Abort(_("unexpected remote reply: %s") % r)
        for d in iter(lambda: fp.read(4096), ""):
            self._send(d)
        self._send("", flush=True)
        return self._pipei

    def _getamount(self):
        l = self._pipei.readline()
        if l == "\n":
            msg = _("check previous remote output")
            self._abort(error.OutOfBandError(hint=msg))
        try:
            return int(l)
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), l))

    def _recv(self):
        return self._pipei.read(self._getamount())

    def _send(self, data, flush=False):
        self._pipeo.write("%d\n" % len(data))
        if data:
            self._pipeo.write(data)
        if flush:
            self._pipeo.flush()


instance = sshpeer
