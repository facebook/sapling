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

import hashlib
import inspect
import os
import re
import socket
import struct
import time

from bindings import commands

from . import commandserver, encoding, error, extensions, pycompat, ui as uimod, util
from .i18n import _


_log = commandserver.log


def _newchgui(srcui, csystem, attachio):
    class chgui(srcui.__class__):
        def __init__(self, src=None):
            super(chgui, self).__init__(src)
            if src:
                self._csystem = getattr(src, "_csystem", csystem)
            else:
                self._csystem = csystem

        def _runsystem(self, cmd, environ, cwd, out):
            # fallback to the original system method if the output needs to be
            # captured (to self._buffers), or the output stream is not stdout
            # (e.g. stderr, cStringIO), because the chg client is not aware of
            # these situations and will behave differently (write to stdout).
            if (
                out is not self.fout
                or not util.safehasattr(self.fout, "fileno")
                or self.fout.fileno() != util.stdout.fileno()
            ):
                return util.system(cmd, environ=environ, cwd=cwd, out=out)
            self.flush()
            return self._csystem.runsystem(cmd, util.shellenviron(environ), cwd)

        def _runpager(self, cmd, env=None):
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

    def __init__(self, in_, out):
        self.in_ = in_
        self.out = out

    def _send_request(self, channel, args):
        data = "\0".join(args) + "\0"
        self.out.write(struct.pack(">cI", channel, len(data)))
        self.out.write(data)
        self.out.flush()

    def _environ_to_args(self, environ):
        return ["%s=%s" % (k, v) for k, v in environ.items()]

    def runsystem(self, cmd, environ, cwd=None):
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
        args = [util.quotecommand(cmd), os.path.abspath(cwd or ".")]
        args.extend(self._environ_to_args(environ))
        self._send_request("s", args)

        length = self.in_.read(4)
        length, = struct.unpack(">I", length)
        if length != 4:
            raise error.Abort(_("invalid response"))
        rc, = struct.unpack(">i", self.in_.read(4))
        return rc

    def runpager(self, cmd, environ, redirectstderr, cmdtable):
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
        args = [util.quotecommand(cmd), redirectsettings]
        args.extend(self._environ_to_args(environ))
        self._send_request("p", args)

        while True:
            cmd = self.in_.readline()[:-1]
            if not cmd:
                break
            if cmd in cmdtable:
                _log("pager subcommand: %s" % cmd)
                cmdtable[cmd]()
            else:
                raise error.Abort(_("unexpected command: %s") % cmd)


_iochannels = [
    # server.ch, ui.fp, mode
    ("cin", "fin", pycompat.sysstr("rb")),
    ("cout", "fout", pycompat.sysstr("wb")),
    ("cerr", "ferr", pycompat.sysstr("wb")),
]


class chgcmdserver(commandserver.server):
    def __init__(self, ui, repo, fin, fout, sock, baseaddress):
        super(chgcmdserver, self).__init__(
            _newchgui(ui, channeledsystem(fin, fout), self.attachio), repo, fin, fout
        )
        self.clientsock = sock
        self._oldios = []  # original (self.ch, ui.fp, fd) before "attachio"
        self.baseaddress = baseaddress

    def cleanup(self):
        super(chgcmdserver, self).cleanup()
        # dispatch._runcatch() does not flush outputs if exception is not
        # handled by dispatch._dispatch()
        self.ui.flush()
        self._restoreio()

    def attachio(self):
        """Attach to client's stdio passed via unix domain socket; all
        channels except cresult will no longer be used
        """
        # tell client to sendmsg() with 1-byte payload, which makes it
        # distinctive from "attachio\n" command consumed by client.read()
        self.clientsock.sendall(struct.pack(">cI", "I", 1))
        clientfds = util.recvfds(self.clientsock.fileno())
        _log("received fds: %r\n" % clientfds)

        ui = self.ui
        ui.flush()
        first = self._saveio()
        for fd, (cn, fn, mode) in zip(clientfds, _iochannels):
            assert fd > 0
            fp = getattr(ui, fn)
            os.dup2(fd, fp.fileno())
            os.close(fd)
            if not first:
                continue
            # reset buffering mode when client is first attached. as we want
            # to see output immediately on pager, the mode stays unchanged
            # when client re-attached. ferr is unchanged because it should
            # be unbuffered no matter if it is a tty or not.
            if fn == "ferr":
                newfp = fp
            else:
                # make it line buffered explicitly because the default is
                # decided on first write(), where fout could be a pager.
                if fp.isatty():
                    bufsize = 1  # line buffered
                else:
                    bufsize = -1  # system default
                try:
                    newfp = util.fdopen(fp.fileno(), mode, bufsize)
                except OSError:
                    # fdopen can fail with EINVAL. For example, run
                    # with nohup. Do not set buffer size in that case.
                    newfp = fp
                setattr(ui, fn, newfp)
            setattr(self, cn, newfp)

        self.cresult.write(struct.pack(">i", len(clientfds)))

    def _saveio(self):
        if self._oldios:
            return False
        ui = self.ui
        for cn, fn, _mode in _iochannels:
            ch = getattr(self, cn)
            fp = getattr(ui, fn)
            fd = os.dup(fp.fileno())
            self._oldios.append((ch, fp, fd))
        return True

    def _restoreio(self):
        ui = self.ui
        for (ch, fp, fd), (cn, fn, _mode) in zip(self._oldios, _iochannels):
            newfp = getattr(ui, fn)
            # close newfp while it's associated with client; otherwise it
            # would be closed when newfp is deleted
            if newfp is not fp:
                newfp.close()
            # restore original fd: fp is open again
            os.dup2(fd, fp.fileno())
            os.close(fd)
            setattr(self, cn, ch)
            setattr(ui, fn, fp)
        del self._oldios[:]

    def chdir(self):
        """Change current directory

        Note that the behavior of --cwd option is bit different from this.
        It does not affect --config parameter.
        """
        path = self._readstr()
        if not path:
            return
        _log("chdir to %r\n" % path)
        os.chdir(path)

    def setumask(self):
        """Change umask"""
        mask = struct.unpack(">I", self._read(4))[0]
        _log("setumask %r\n" % mask)
        os.umask(mask)

    def runcommand(self):
        # Environment variables might change, reload env.
        util._reloadenv()
        args = self._readlist()
        pycompat.sysargv[1:] = args
        origui = uimod.ui
        # Use the class patched by _newchgui so 'system' and 'pager' requests
        # get forwarded to chg client
        uimod.ui = self.ui.__class__
        try:
            ret = commands.run(
                [pycompat.sysargv[0]] + args, self.ui.fin, self.ui.fout, self.ui.ferr
            )
            self.cresult.write(struct.pack(">i", int(ret & 255)))
        finally:
            uimod.ui = origui

    def setenv(self):
        """Clear and update os.environ

        Note that not all variables can make an effect on the running process.
        """
        l = self._readlist()
        try:
            newenv = dict(s.split("=", 1) for s in l if "=" in s)
        except ValueError:
            raise ValueError("unexpected value in setenv request")
        _log("setenv: %r\n" % sorted(newenv.keys()))
        encoding.environ.clear()
        encoding.environ.update(newenv)

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
            util.setprocname(name)

        capabilities["setprocname"] = setprocname


def _tempaddress(address):
    return "%s.%d.tmp" % (address, os.getpid())


def _realaddress(address):
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
        self._socketstat = os.stat(tempaddress)
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
            stat = os.stat(self._realaddress)
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
