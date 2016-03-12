# commandserver.py - communicate with Mercurial's API over a pipe
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import SocketServer
import errno
import os
import struct
import sys
import traceback

from .i18n import _
from . import (
    encoding,
    error,
    util,
)

logfile = None

def log(*args):
    if not logfile:
        return

    for a in args:
        logfile.write(str(a))

    logfile.flush()

class channeledoutput(object):
    """
    Write data to out in the following format:

    data length (unsigned int),
    data
    """
    def __init__(self, out, channel):
        self.out = out
        self.channel = channel

    @property
    def name(self):
        return '<%c-channel>' % self.channel

    def write(self, data):
        if not data:
            return
        self.out.write(struct.pack('>cI', self.channel, len(data)))
        self.out.write(data)
        self.out.flush()

    def __getattr__(self, attr):
        if attr in ('isatty', 'fileno', 'tell', 'seek'):
            raise AttributeError(attr)
        return getattr(self.out, attr)

class channeledinput(object):
    """
    Read data from in_.

    Requests for input are written to out in the following format:
    channel identifier - 'I' for plain input, 'L' line based (1 byte)
    how many bytes to send at most (unsigned int),

    The client replies with:
    data length (unsigned int), 0 meaning EOF
    data
    """

    maxchunksize = 4 * 1024

    def __init__(self, in_, out, channel):
        self.in_ = in_
        self.out = out
        self.channel = channel

    @property
    def name(self):
        return '<%c-channel>' % self.channel

    def read(self, size=-1):
        if size < 0:
            # if we need to consume all the clients input, ask for 4k chunks
            # so the pipe doesn't fill up risking a deadlock
            size = self.maxchunksize
            s = self._read(size, self.channel)
            buf = s
            while s:
                s = self._read(size, self.channel)
                buf += s

            return buf
        else:
            return self._read(size, self.channel)

    def _read(self, size, channel):
        if not size:
            return ''
        assert size > 0

        # tell the client we need at most size bytes
        self.out.write(struct.pack('>cI', channel, size))
        self.out.flush()

        length = self.in_.read(4)
        length = struct.unpack('>I', length)[0]
        if not length:
            return ''
        else:
            return self.in_.read(length)

    def readline(self, size=-1):
        if size < 0:
            size = self.maxchunksize
            s = self._read(size, 'L')
            buf = s
            # keep asking for more until there's either no more or
            # we got a full line
            while s and s[-1] != '\n':
                s = self._read(size, 'L')
                buf += s

            return buf
        else:
            return self._read(size, 'L')

    def __iter__(self):
        return self

    def next(self):
        l = self.readline()
        if not l:
            raise StopIteration
        return l

    def __getattr__(self, attr):
        if attr in ('isatty', 'fileno', 'tell', 'seek'):
            raise AttributeError(attr)
        return getattr(self.in_, attr)

class server(object):
    """
    Listens for commands on fin, runs them and writes the output on a channel
    based stream to fout.
    """
    def __init__(self, ui, repo, fin, fout):
        self.cwd = os.getcwd()

        # developer config: cmdserver.log
        logpath = ui.config("cmdserver", "log", None)
        if logpath:
            global logfile
            if logpath == '-':
                # write log on a special 'd' (debug) channel
                logfile = channeledoutput(fout, 'd')
            else:
                logfile = open(logpath, 'a')

        if repo:
            # the ui here is really the repo ui so take its baseui so we don't
            # end up with its local configuration
            self.ui = repo.baseui
            self.repo = repo
            self.repoui = repo.ui
        else:
            self.ui = ui
            self.repo = self.repoui = None

        self.cerr = channeledoutput(fout, 'e')
        self.cout = channeledoutput(fout, 'o')
        self.cin = channeledinput(fin, fout, 'I')
        self.cresult = channeledoutput(fout, 'r')

        self.client = fin

    def _read(self, size):
        if not size:
            return ''

        data = self.client.read(size)

        # is the other end closed?
        if not data:
            raise EOFError

        return data

    def _readstr(self):
        """read a string from the channel

        format:
        data length (uint32), data
        """
        length = struct.unpack('>I', self._read(4))[0]
        if not length:
            return ''
        return self._read(length)

    def _readlist(self):
        """read a list of NULL separated strings from the channel"""
        s = self._readstr()
        if s:
            return s.split('\0')
        else:
            return []

    def runcommand(self):
        """ reads a list of \0 terminated arguments, executes
        and writes the return code to the result channel """
        from . import dispatch  # avoid cycle

        args = self._readlist()

        # copy the uis so changes (e.g. --config or --verbose) don't
        # persist between requests
        copiedui = self.ui.copy()
        uis = [copiedui]
        if self.repo:
            self.repo.baseui = copiedui
            # clone ui without using ui.copy because this is protected
            repoui = self.repoui.__class__(self.repoui)
            repoui.copy = copiedui.copy # redo copy protection
            uis.append(repoui)
            self.repo.ui = self.repo.dirstate._ui = repoui
            self.repo.invalidateall()

        # reset last-print time of progress bar per command
        # (progbar is singleton, we don't have to do for all uis)
        if copiedui._progbar:
            copiedui._progbar.resetstate()

        for ui in uis:
            # any kind of interaction must use server channels, but chg may
            # replace channels by fully functional tty files. so nontty is
            # enforced only if cin is a channel.
            if not util.safehasattr(self.cin, 'fileno'):
                ui.setconfig('ui', 'nontty', 'true', 'commandserver')

        req = dispatch.request(args[:], copiedui, self.repo, self.cin,
                               self.cout, self.cerr)

        ret = (dispatch.dispatch(req) or 0) & 255 # might return None

        # restore old cwd
        if '--cwd' in args:
            os.chdir(self.cwd)

        self.cresult.write(struct.pack('>i', int(ret)))

    def getencoding(self):
        """ writes the current encoding to the result channel """
        self.cresult.write(encoding.encoding)

    def serveone(self):
        cmd = self.client.readline()[:-1]
        if cmd:
            handler = self.capabilities.get(cmd)
            if handler:
                handler(self)
            else:
                # clients are expected to check what commands are supported by
                # looking at the servers capabilities
                raise error.Abort(_('unknown command %s') % cmd)

        return cmd != ''

    capabilities = {'runcommand'  : runcommand,
                    'getencoding' : getencoding}

    def serve(self):
        hellomsg = 'capabilities: ' + ' '.join(sorted(self.capabilities))
        hellomsg += '\n'
        hellomsg += 'encoding: ' + encoding.encoding
        hellomsg += '\n'
        hellomsg += 'pid: %d' % util.getpid()

        # write the hello msg in -one- chunk
        self.cout.write(hellomsg)

        try:
            while self.serveone():
                pass
        except EOFError:
            # we'll get here if the client disconnected while we were reading
            # its request
            return 1

        return 0

def _protectio(ui):
    """ duplicates streams and redirect original to null if ui uses stdio """
    ui.flush()
    newfiles = []
    nullfd = os.open(os.devnull, os.O_RDWR)
    for f, sysf, mode in [(ui.fin, sys.stdin, 'rb'),
                          (ui.fout, sys.stdout, 'wb')]:
        if f is sysf:
            newfd = os.dup(f.fileno())
            os.dup2(nullfd, f.fileno())
            f = os.fdopen(newfd, mode)
        newfiles.append(f)
    os.close(nullfd)
    return tuple(newfiles)

def _restoreio(ui, fin, fout):
    """ restores streams from duplicated ones """
    ui.flush()
    for f, uif in [(fin, ui.fin), (fout, ui.fout)]:
        if f is not uif:
            os.dup2(f.fileno(), uif.fileno())
            f.close()

class pipeservice(object):
    def __init__(self, ui, repo, opts):
        self.ui = ui
        self.repo = repo

    def init(self):
        pass

    def run(self):
        ui = self.ui
        # redirect stdio to null device so that broken extensions or in-process
        # hooks will never cause corruption of channel protocol.
        fin, fout = _protectio(ui)
        try:
            sv = server(ui, self.repo, fin, fout)
            return sv.serve()
        finally:
            _restoreio(ui, fin, fout)

class _requesthandler(SocketServer.StreamRequestHandler):
    def handle(self):
        ui = self.server.ui
        repo = self.server.repo
        sv = None
        try:
            sv = server(ui, repo, self.rfile, self.wfile)
            try:
                sv.serve()
            # handle exceptions that may be raised by command server. most of
            # known exceptions are caught by dispatch.
            except error.Abort as inst:
                ui.warn(_('abort: %s\n') % inst)
            except IOError as inst:
                if inst.errno != errno.EPIPE:
                    raise
            except KeyboardInterrupt:
                pass
        except: # re-raises
            # also write traceback to error channel. otherwise client cannot
            # see it because it is written to server's stderr by default.
            if sv:
                cerr = sv.cerr
            else:
                cerr = channeledoutput(self.wfile, 'e')
            traceback.print_exc(file=cerr)
            raise

class unixservice(object):
    """
    Listens on unix domain socket and forks server per connection
    """
    def __init__(self, ui, repo, opts):
        self.ui = ui
        self.repo = repo
        self.address = opts['address']
        if not util.safehasattr(SocketServer, 'UnixStreamServer'):
            raise error.Abort(_('unsupported platform'))
        if not self.address:
            raise error.Abort(_('no socket path specified with --address'))

    def init(self):
        class cls(SocketServer.ForkingMixIn, SocketServer.UnixStreamServer):
            ui = self.ui
            repo = self.repo
        self.server = cls(self.address, _requesthandler)
        self.ui.status(_('listening at %s\n') % self.address)
        self.ui.flush()  # avoid buffering of status message

    def run(self):
        try:
            self.server.serve_forever()
        finally:
            os.unlink(self.address)

_servicemap = {
    'pipe': pipeservice,
    'unix': unixservice,
    }

def createservice(ui, repo, opts):
    mode = opts['cmdserver']
    try:
        return _servicemap[mode](ui, repo, opts)
    except KeyError:
        raise error.Abort(_('unknown mode %s') % mode)
