# chgserver.py - command server extension for cHg
#
# Copyright 2011 Yuya Nishihara <yuya@tcha.org>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""command server extension for cHg (EXPERIMENTAL)

'S' channel (read/write)
    propagate ui.system() request to client

'attachio' command
    attach client's stdio passed by sendmsg()

'chdir' command
    change current directory

'getpager' command
    checks if pager is enabled and which pager should be executed

'setenv' command
    replace os.environ completely

'SIGHUP' signal
    reload configuration files
"""

from __future__ import absolute_import

import SocketServer
import errno
import os
import re
import signal
import struct
import traceback

from mercurial.i18n import _

from mercurial import (
    cmdutil,
    commands,
    commandserver,
    dispatch,
    error,
    osutil,
    util,
)

# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

_log = commandserver.log

# copied from hgext/pager.py:uisetup()
def _setuppagercmd(ui, options, cmd):
    if not ui.formatted():
        return

    p = ui.config("pager", "pager", os.environ.get("PAGER"))
    usepager = False
    always = util.parsebool(options['pager'])
    auto = options['pager'] == 'auto'

    if not p:
        pass
    elif always:
        usepager = True
    elif not auto:
        usepager = False
    else:
        attended = ['annotate', 'cat', 'diff', 'export', 'glog', 'log', 'qdiff']
        attend = ui.configlist('pager', 'attend', attended)
        ignore = ui.configlist('pager', 'ignore')
        cmds, _ = cmdutil.findcmd(cmd, commands.table)

        for cmd in cmds:
            var = 'attend-%s' % cmd
            if ui.config('pager', var):
                usepager = ui.configbool('pager', var)
                break
            if (cmd in attend or
                (cmd not in ignore and not attend)):
                usepager = True
                break

    if usepager:
        ui.setconfig('ui', 'formatted', ui.formatted(), 'pager')
        ui.setconfig('ui', 'interactive', False, 'pager')
        return p

_envvarre = re.compile(r'\$[a-zA-Z_]+')

def _clearenvaliases(cmdtable):
    """Remove stale command aliases referencing env vars; variable expansion
    is done at dispatch.addaliases()"""
    for name, tab in cmdtable.items():
        cmddef = tab[0]
        if (isinstance(cmddef, dispatch.cmdalias) and
            not cmddef.definition.startswith('!') and  # shell alias
            _envvarre.search(cmddef.definition)):
            del cmdtable[name]

def _newchgui(srcui, csystem):
    class chgui(srcui.__class__):
        def __init__(self, src=None):
            super(chgui, self).__init__(src)
            if src:
                self._csystem = getattr(src, '_csystem', csystem)
            else:
                self._csystem = csystem

        def system(self, cmd, environ=None, cwd=None, onerr=None,
                   errprefix=None):
            # copied from mercurial/util.py:system()
            self.flush()
            def py2shell(val):
                if val is None or val is False:
                    return '0'
                if val is True:
                    return '1'
                return str(val)
            env = os.environ.copy()
            if environ:
                env.update((k, py2shell(v)) for k, v in environ.iteritems())
            env['HG'] = util.hgexecutable()
            rc = self._csystem(cmd, env, cwd)
            if rc and onerr:
                errmsg = '%s %s' % (os.path.basename(cmd.split(None, 1)[0]),
                                    util.explainexit(rc)[0])
                if errprefix:
                    errmsg = '%s: %s' % (errprefix, errmsg)
                raise onerr(errmsg)
            return rc

    return chgui(srcui)

def _renewui(srcui):
    newui = srcui.__class__()
    for a in ['fin', 'fout', 'ferr', 'environ']:
        setattr(newui, a, getattr(srcui, a))
    if util.safehasattr(srcui, '_csystem'):
        newui._csystem = srcui._csystem
    # stolen from tortoisehg.util.copydynamicconfig()
    for section, name, value in srcui.walkconfig():
        source = srcui.configsource(section, name)
        if ':' in source:
            # path:line
            continue
        if source == 'none':
            # ui.configsource returns 'none' by default
            source = ''
        newui.setconfig(section, name, value, source)
    return newui

class channeledsystem(object):
    """Propagate ui.system() request in the following format:

    payload length (unsigned int),
    cmd, '\0',
    cwd, '\0',
    envkey, '=', val, '\0',
    ...
    envkey, '=', val

    and waits:

    exitcode length (unsigned int),
    exitcode (int)
    """
    def __init__(self, in_, out, channel):
        self.in_ = in_
        self.out = out
        self.channel = channel

    def __call__(self, cmd, environ, cwd):
        args = [util.quotecommand(cmd), cwd or '.']
        args.extend('%s=%s' % (k, v) for k, v in environ.iteritems())
        data = '\0'.join(args)
        self.out.write(struct.pack('>cI', self.channel, len(data)))
        self.out.write(data)
        self.out.flush()

        length = self.in_.read(4)
        length, = struct.unpack('>I', length)
        if length != 4:
            raise error.Abort(_('invalid response'))
        rc, = struct.unpack('>i', self.in_.read(4))
        return rc

_iochannels = [
    # server.ch, ui.fp, mode
    ('cin', 'fin', 'rb'),
    ('cout', 'fout', 'wb'),
    ('cerr', 'ferr', 'wb'),
]

class chgcmdserver(commandserver.server):
    def __init__(self, ui, repo, fin, fout, sock):
        super(chgcmdserver, self).__init__(
            _newchgui(ui, channeledsystem(fin, fout, 'S')), repo, fin, fout)
        self.clientsock = sock
        self._oldios = []  # original (self.ch, ui.fp, fd) before "attachio"

    def cleanup(self):
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
        self.clientsock.sendall(struct.pack('>cI', 'I', 1))
        clientfds = osutil.recvfds(self.clientsock.fileno())
        _log('received fds: %r\n' % clientfds)

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
            if fn == 'ferr':
                newfp = fp
            else:
                # make it line buffered explicitly because the default is
                # decided on first write(), where fout could be a pager.
                if fp.isatty():
                    bufsize = 1  # line buffered
                else:
                    bufsize = -1  # system default
                newfp = os.fdopen(fp.fileno(), mode, bufsize)
                setattr(ui, fn, newfp)
            setattr(self, cn, newfp)

        self.cresult.write(struct.pack('>i', len(clientfds)))

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
        length = struct.unpack('>I', self._read(4))[0]
        if not length:
            return
        path = self._read(length)
        _log('chdir to %r\n' % path)
        os.chdir(path)

    def getpager(self):
        """Read cmdargs and write pager command to r-channel if enabled

        If pager isn't enabled, this writes '\0' because channeledoutput
        does not allow to write empty data.
        """
        length = struct.unpack('>I', self._read(4))[0]
        if not length:
            args = []
        else:
            args = self._read(length).split('\0')
        try:
            cmd, _func, args, options, _cmdoptions = dispatch._parse(self.ui,
                                                                     args)
        except (error.Abort, error.AmbiguousCommand, error.CommandError,
                error.UnknownCommand):
            cmd = None
            options = {}
        if not cmd or 'pager' not in options:
            self.cresult.write('\0')
            return

        pagercmd = _setuppagercmd(self.ui, options, cmd)
        if pagercmd:
            self.cresult.write(pagercmd)
        else:
            self.cresult.write('\0')

    def setenv(self):
        """Clear and update os.environ

        Note that not all variables can make an effect on the running process.
        """
        length = struct.unpack('>I', self._read(4))[0]
        if not length:
            return
        s = self._read(length)
        try:
            newenv = dict(l.split('=', 1) for l in s.split('\0'))
        except ValueError:
            raise ValueError('unexpected value in setenv request')

        diffkeys = set(k for k in set(os.environ.keys() + newenv.keys())
                       if os.environ.get(k) != newenv.get(k))
        _log('change env: %r\n' % sorted(diffkeys))

        os.environ.clear()
        os.environ.update(newenv)

        if set(['HGPLAIN', 'HGPLAINEXCEPT']) & diffkeys:
            # reload config so that ui.plain() takes effect
            self.ui = _renewui(self.ui)

        _clearenvaliases(commands.table)

    capabilities = commandserver.server.capabilities.copy()
    capabilities.update({'attachio': attachio,
                         'chdir': chdir,
                         'getpager': getpager,
                         'setenv': setenv})

# copied from mercurial/commandserver.py
class _requesthandler(SocketServer.StreamRequestHandler):
    def handle(self):
        ui = self.server.ui
        repo = self.server.repo
        sv = chgcmdserver(ui, repo, self.rfile, self.wfile, self.connection)
        try:
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
            finally:
                sv.cleanup()
        except: # re-raises
            # also write traceback to error channel. otherwise client cannot
            # see it because it is written to server's stderr by default.
            traceback.print_exc(file=sv.cerr)
            raise

class chgunixservice(commandserver.unixservice):
    def init(self):
        # drop options set for "hg serve --cmdserver" command
        self.ui.setconfig('progress', 'assume-tty', None)
        signal.signal(signal.SIGHUP, self._reloadconfig)
        class cls(SocketServer.ForkingMixIn, SocketServer.UnixStreamServer):
            ui = self.ui
            repo = self.repo
        self.server = cls(self.address, _requesthandler)
        # avoid writing "listening at" message to stdout before attachio
        # request, which calls setvbuf()

    def _reloadconfig(self, signum, frame):
        self.ui = self.server.ui = _renewui(self.ui)

def uisetup(ui):
    commandserver._servicemap['chgunix'] = chgunixservice
