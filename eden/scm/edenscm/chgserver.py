# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# chgserver.py - command server extension for cHg
#
# Copyright 2011 Yuya Nishihara <yuya@tcha.org>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""command server extension for cHg

'S' channel (read/write)
    propagate ui.system() request to client

'attachio' command
    attach client's stdio passed by sendmsg()

'chdir' command
    change current directory

'setenv' command
    replace os.environ completely

'setumask' command
    set umask

'validate' command
    reload the config and check if the server is up to date

Config
------

::

  [chgserver]
  # how long (in seconds) should an idle chg server exit
  idletimeout = 3600

  # whether to skip config or env change checks
  skiphash = False
"""

from __future__ import absolute_import

import os
import socket
import struct
import time
from typing import BinaryIO, Callable, Dict, List, Optional

from bindings import commands, hgtime
from edenscm import tracing

from . import commandserver, encoding, error, pycompat, ui as uimod, util
from .i18n import _


_log = commandserver.log


def _newchgui(srcui, csystem, attachio):
    class chgui(srcui.__class__):
        def __init__(self, src=None, rcfg=None):
            super(chgui, self).__init__(src, rcfg)
            if src:
                self._csystem = getattr(src, "_csystem", csystem)
            else:
                self._csystem = csystem

        def _runsystem(self, cmd, environ, cwd, out):
            # fallback to the original system method if the output needs to be
            # captured (to self._buffers), or the output stream is not stdout
            # (e.g. stderr, cStringIO), because the chg client is not aware of
            # these situations and will behave differently (write to stdout).
            if out is not self.fout or self._buffers:
                return util.rawsystem(cmd, environ=environ, cwd=cwd, out=out)
            self.flush()
            return self._csystem.runsystem(cmd, util.shellenviron(environ), cwd)

        def _runpager(self, cmd, env=None):
            util.mainio.disable_progress()
            self._csystem.runpager(
                cmd,
                util.shellenviron(env),
                redirectstderr=self.configbool("pager", "stderr"),
                cmdtable={"attachio": attachio},
            )
            return True

    return chgui(srcui)


class channeledsystem(object):
    """Propagate ui.system() and ui._runpager() requests to the chg client"""

    def __init__(self, in_: "BinaryIO", out: "BinaryIO") -> None:
        self.in_ = in_
        self.out = out

    def _send_request(self, channel: bytes, args: "List[str]") -> None:
        data = pycompat.encodeutf8("\0".join(args) + "\0")
        self.out.write(struct.pack(">cI", channel, len(data)))
        self.out.write(data)
        self.out.flush()

    def _environ_to_args(self, environ: "Dict[str, str]") -> "List[str]":
        return ["%s=%s" % (k, v) for k, v in environ.items()]

    def runsystem(
        self, cmd: str, environ: "Dict[str, str]", cwd: "Optional[str]" = None
    ) -> int:
        """Send a request to run a system command.

        This request type is sent with the 's' channel code.
        The request contents are a series of null-terminated strings:
        - the first string is the command string, to be run with "sh -c"
        - the second string is the working directory to use for the command
        - all remaining arguments are environment variables, all in the form
          "name=value"

        After sending a system request, the server waits for

        exitcode length (unsigned int),
        exitcode (int)"""
        args = [cmd, os.path.abspath(cwd or ".")]
        args.extend(self._environ_to_args(environ))
        self._send_request(b"s", args)

        length = self.in_.read(4)
        (length,) = struct.unpack(">I", length)
        if length != 4:
            raise error.Abort(_("invalid response"))
        (rc,) = struct.unpack(">i", self.in_.read(4))
        return rc

    def runpager(
        self,
        pagercmd: str,
        environ: "Dict[str, str]",
        redirectstderr: "BinaryIO",
        cmdtable: "Dict[str, Callable]",
    ) -> None:
        """Requests to run a pager command are sent using the 'p' channel code.
        The request contents are a series of null-terminated strings:
        - the first string is the pager command string, to be run with "sh -c"
        - the second string indicates desired I/O redirection settings
        - all remaining arguments are environment variables, all in the form
          "name=value"

        After sending a pager request the server repeatedly waits for a command name
        ending with '\n' and executes it defined by cmdtable, or exits the loop if the
        command name is empty.
        """
        redirectsettings = "stderr" if redirectstderr else ""
        args = [pagercmd, redirectsettings]
        args.extend(self._environ_to_args(environ))
        self._send_request(b"p", args)

        while True:
            bcmd = self.in_.readline()[:-1]
            cmd = pycompat.decodeutf8(bcmd)
            if not cmd:
                break
            if cmd in cmdtable:
                _log("pager subcommand: %s" % cmd)
                cmdtable[cmd]()
            else:
                raise error.Abort(_("unexpected command: %s") % cmd)


_iochannels = [
    # server.ch, fileno, mode
    ("cin", 0, "rb"),
    ("cout", 1, "wb"),
    ("cerr", 2, "wb"),
]


class chgcmdserver(commandserver.server):
    def __init__(self, ui, repo, fin, fout, sock, baseaddress):
        super(chgcmdserver, self).__init__(
            _newchgui(ui, channeledsystem(fin, fout), self.attachio), repo, fin, fout
        )
        self.clientsock = sock
        self._ioattached = False
        self.baseaddress = baseaddress

    def cleanup(self):
        super(chgcmdserver, self).cleanup()
        # dispatch._runcatch() does not flush outputs if exception is not
        # handled by dispatch._dispatch()
        self.ui.flush()

    def attachio(self) -> None:
        """Attach to client's stdio passed via unix domain socket; all
        channels except cresult will no longer be used
        """
        # tell client to sendmsg() with 1-byte payload, which makes it
        # distinctive from "attachio\n" command consumed by client.read()
        self.clientsock.sendall(struct.pack(">cI", b"I", 1))
        clientfds = util.recvfds(self.clientsock.fileno())
        _log("received fds: %r\n" % clientfds)

        ui = self.ui
        ui.flush()
        for fd, (cn, fileno, mode) in zip(clientfds, _iochannels):
            # Changing the raw fds directly.
            # This will affect Rust std::io::{Stdin, Stdout, Stderr}.
            assert fd > 0
            os.dup2(fd, fileno)
            os.close(fd)

        self.cresult.write(struct.pack(">i", len(clientfds)))
        self._ioattached = True

    def chdir(self) -> None:
        """Change current directory

        Note that the behavior of --cwd option is bit different from this.
        It does not affect --config parameter.
        """
        path = self._readstr()
        if not path:
            return
        path = pycompat.decodeutf8(path)
        _log("chdir to %r\n" % path)
        os.chdir(path)

    def setumask(self) -> None:
        """Change umask"""
        mask = struct.unpack(">I", self._read(4))[0]
        _log("setumask %r\n" % mask)
        os.umask(mask)

    def runcommand(self):
        # type () -> None
        # Environment variables might change, reload env.

        # Re-enable tracing after forking.
        tracing.disabletracing = False

        util._reloadenv()
        args = self._readlist()
        pycompat.sysargv[1:] = args
        origui = uimod.ui
        # Use the class patched by _newchgui so 'system' and 'pager' requests
        # get forwarded to chg client
        uimod.ui = self.ui.__class__
        try:
            ret = commands.run([pycompat.sysargv[0]] + args)
            self.cresult.write(struct.pack(">i", int(ret & 255)))
        finally:
            uimod.ui = origui

    def setenv(self) -> None:
        """Clear and update os.environ

        Note that not all variables can make an effect on the running process.
        """
        l = self._readlist()
        try:
            newenv = dict(  # ignore below bc pyre doesn't like list to kv conversion
                s.split("=", 1) for s in l if "=" in s
            )
        except ValueError:
            raise ValueError("unexpected value in setenv request")
        _log("setenv: %r\n" % sorted(newenv.keys()))
        encoding.environ.clear()
        encoding.environ.update(newenv)
        # Apply $TZ changes.
        hgtime.tzset()

    capabilities = commandserver.server.capabilities.copy()
    capabilities.update(
        {
            "attachio": attachio,
            "chdir": chdir,
            "runcommand": runcommand,
            "setenv": setenv,
            "setumask": setumask,
        }
    )

    if util.safehasattr(util, "setprocname"):

        def setprocname(self):
            """Change process title"""
            name = self._readstr()
            _log("setprocname: %r\n" % name)
            util.setprocname(pycompat.decodeutf8(name))

        # pyre-fixme[16]: `chgcmdserver` has no attribute `setprocname`.
        capabilities["setprocname"] = setprocname


def _tempaddress(address: str) -> str:
    return "%s.%d.tmp" % (address, os.getpid())


def _realaddress(address: str) -> str:
    # if the basename of address contains '.', use only the left part. this
    # makes it possible for the client to pass 'server.tmp$PID' and follow by
    # an atomic rename to avoid locking when spawning new servers.
    dirname, basename = os.path.split(address)
    basename = basename.split(".", 1)[0]
    return os.path.join(dirname, basename)


class chgunixservicehandler(object):
    """Set of operations for chg services"""

    pollinterval = 1  # [sec]

    def __init__(self, ui):
        self.ui = ui
        self._idletimeout = ui.configint("chgserver", "idletimeout")
        self._lastactive = time.time()

    def bindsocket(self, sock, address):
        self._baseaddress = address
        self._realaddress = _realaddress(address)
        self._bind(sock)
        self._createsymlink()
        # no "listening at" message should be printed to simulate hg behavior

    def _bind(self, sock):
        # use a unique temp address so we can stat the file and do ownership
        # check later
        tempaddress = _tempaddress(self._realaddress)
        util.bindunixsocket(sock, tempaddress)
        self._socketstat = util.stat(tempaddress)
        sock.listen(socket.SOMAXCONN)
        # rename will replace the old socket file if exists atomically. the
        # old server will detect ownership change and exit.
        util.rename(tempaddress, self._realaddress)

    def _createsymlink(self):
        if self._baseaddress == self._realaddress:
            return
        tempaddress = _tempaddress(self._baseaddress)
        os.symlink(os.path.basename(self._realaddress), tempaddress)
        util.rename(tempaddress, self._baseaddress)

    def _issocketowner(self):
        try:
            stat = util.stat(self._realaddress)
            return (
                stat.st_ino == self._socketstat.st_ino
                and stat.st_mtime == self._socketstat.st_mtime
            )
        except OSError:
            return False

    def unlinksocket(self, address):
        if not self._issocketowner():
            return
        # it is possible to have a race condition here that we may
        # remove another server's socket file. but that's okay
        # since that server will detect and exit automatically and
        # the client will start a new server on demand.
        util.tryunlink(self._realaddress)

    def shouldexit(self):
        if not self._issocketowner():
            self.ui.debug("%s is not owned, exiting.\n" % self._realaddress)
            return True
        if time.time() - self._lastactive > self._idletimeout:
            self.ui.debug("being idle too long. exiting.\n")
            return True
        return False

    def newconnection(self):
        self._lastactive = time.time()

    def createcmdserver(self, repo, conn, fin, fout):
        return chgcmdserver(self.ui, repo, fin, fout, conn, self._baseaddress)


def chgunixservice(ui, repo, opts):
    # CHGINTERNALMARK is set by chg client. It is an indication of things are
    # started by chg so other code can do things accordingly, like disabling
    # demandimport or detecting chg client started by chg client. When executed
    # here, CHGINTERNALMARK is no longer useful and hence dropped to make
    # environ cleaner.
    if "CHGINTERNALMARK" in encoding.environ:
        del encoding.environ["CHGINTERNALMARK"]

    if repo:
        # one chgserver can serve multiple repos. drop repo information
        ui.setconfig("bundle", "mainreporoot", "", "repo")
    h = chgunixservicehandler(ui)
    return commandserver.unixforkingservice(ui, repo=None, opts=opts, handler=h)
