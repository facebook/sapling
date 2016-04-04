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

'setumask' command
    set umask

'validate' command
    reload the config and check if the server is up to date

Config
------

::

  [chgserver]
  idletimeout = 3600 # seconds, after which an idle server will exit
  skiphash = False   # whether to skip config or env change checks
"""

from __future__ import absolute_import

import SocketServer
import errno
import gc
import inspect
import os
import random
import re
import struct
import sys
import threading
import time
import traceback

from mercurial.i18n import _

from mercurial import (
    cmdutil,
    commands,
    commandserver,
    dispatch,
    error,
    extensions,
    osutil,
    util,
)

# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

_log = commandserver.log

def _hashlist(items):
    """return sha1 hexdigest for a list"""
    return util.sha1(str(items)).hexdigest()

# sensitive config sections affecting confighash
_configsections = [
    'extdiff',  # uisetup will register new commands
    'extensions',
]

# sensitive environment variables affecting confighash
_envre = re.compile(r'''\A(?:
                    CHGHG
                    |HG.*
                    |LANG(?:UAGE)?
                    |LC_.*
                    |LD_.*
                    |PATH
                    |PYTHON.*
                    |TERM(?:INFO)?
                    |TZ
                    )\Z''', re.X)

def _confighash(ui):
    """return a quick hash for detecting config/env changes

    confighash is the hash of sensitive config items and environment variables.

    for chgserver, it is designed that once confighash changes, the server is
    not qualified to serve its client and should redirect the client to a new
    server. different from mtimehash, confighash change will not mark the
    server outdated and exit since the user can have different configs at the
    same time.
    """
    sectionitems = []
    for section in _configsections:
        sectionitems.append(ui.configitems(section))
    sectionhash = _hashlist(sectionitems)
    envitems = [(k, v) for k, v in os.environ.iteritems() if _envre.match(k)]
    envhash = _hashlist(sorted(envitems))
    return sectionhash[:6] + envhash[:6]

def _getmtimepaths(ui):
    """get a list of paths that should be checked to detect change

    The list will include:
    - extensions (will not cover all files for complex extensions)
    - mercurial/__version__.py
    - python binary
    """
    modules = [m for n, m in extensions.extensions(ui)]
    try:
        from mercurial import __version__
        modules.append(__version__)
    except ImportError:
        pass
    files = [sys.executable]
    for m in modules:
        try:
            files.append(inspect.getabsfile(m))
        except TypeError:
            pass
    return sorted(set(files))

def _mtimehash(paths):
    """return a quick hash for detecting file changes

    mtimehash calls stat on given paths and calculate a hash based on size and
    mtime of each file. mtimehash does not read file content because reading is
    expensive. therefore it's not 100% reliable for detecting content changes.
    it's possible to return different hashes for same file contents.
    it's also possible to return a same hash for different file contents for
    some carefully crafted situation.

    for chgserver, it is designed that once mtimehash changes, the server is
    considered outdated immediately and should no longer provide service.
    """
    def trystat(path):
        try:
            st = os.stat(path)
            return (st.st_mtime, st.st_size)
        except OSError:
            # could be ENOENT, EPERM etc. not fatal in any case
            pass
    return _hashlist(map(trystat, paths))[:12]

class hashstate(object):
    """a structure storing confighash, mtimehash, paths used for mtimehash"""
    def __init__(self, confighash, mtimehash, mtimepaths):
        self.confighash = confighash
        self.mtimehash = mtimehash
        self.mtimepaths = mtimepaths

    @staticmethod
    def fromui(ui, mtimepaths=None):
        if mtimepaths is None:
            mtimepaths = _getmtimepaths(ui)
        confighash = _confighash(ui)
        mtimehash = _mtimehash(mtimepaths)
        _log('confighash = %s mtimehash = %s\n' % (confighash, mtimehash))
        return hashstate(confighash, mtimehash, mtimepaths)

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
            # fallback to the original system method if the output needs to be
            # captured (to self._buffers), or the output stream is not stdout
            # (e.g. stderr, cStringIO), because the chg client is not aware of
            # these situations and will behave differently (write to stdout).
            if (any(s[1] for s in self._bufferstates)
                or not util.safehasattr(self.fout, 'fileno')
                or self.fout.fileno() != sys.stdout.fileno()):
                return super(chgui, self).system(cmd, environ, cwd, onerr,
                                                 errprefix)
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

def _loadnewui(srcui, args):
    newui = srcui.__class__()
    for a in ['fin', 'fout', 'ferr', 'environ']:
        setattr(newui, a, getattr(srcui, a))
    if util.safehasattr(srcui, '_csystem'):
        newui._csystem = srcui._csystem

    # internal config: extensions.chgserver
    newui.setconfig('extensions', 'chgserver',
                    srcui.config('extensions', 'chgserver'), '--config')

    # command line args
    args = args[:]
    dispatch._parseconfig(newui, dispatch._earlygetopt(['--config'], args))

    # stolen from tortoisehg.util.copydynamicconfig()
    for section, name, value in srcui.walkconfig():
        source = srcui.configsource(section, name)
        if ':' in source or source == '--config':
            # path:line or command line
            continue
        if source == 'none':
            # ui.configsource returns 'none' by default
            source = ''
        newui.setconfig(section, name, value, source)

    # load wd and repo config, copied from dispatch.py
    cwds = dispatch._earlygetopt(['--cwd'], args)
    cwd = cwds and os.path.realpath(cwds[-1]) or None
    rpath = dispatch._earlygetopt(["-R", "--repository", "--repo"], args)
    path, newlui = dispatch._getlocal(newui, rpath, wd=cwd)

    return (newui, newlui)

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
        args = [util.quotecommand(cmd), os.path.abspath(cwd or '.')]
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
    def __init__(self, ui, repo, fin, fout, sock, hashstate, baseaddress):
        super(chgcmdserver, self).__init__(
            _newchgui(ui, channeledsystem(fin, fout, 'S')), repo, fin, fout)
        self.clientsock = sock
        self._oldios = []  # original (self.ch, ui.fp, fd) before "attachio"
        self.hashstate = hashstate
        self.baseaddress = baseaddress
        if hashstate is not None:
            self.capabilities = self.capabilities.copy()
            self.capabilities['validate'] = chgcmdserver.validate

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

    def validate(self):
        """Reload the config and check if the server is up to date

        Read a list of '\0' separated arguments.
        Write a non-empty list of '\0' separated instruction strings or '\0'
        if the list is empty.
        An instruction string could be either:
            - "unlink $path", the client should unlink the path to stop the
              outdated server.
            - "redirect $path", the client should attempt to connect to $path
              first. If it does not work, start a new server. It implies
              "reconnect".
            - "exit $n", the client should exit directly with code n.
              This may happen if we cannot parse the config.
            - "reconnect", the client should close the connection and
              reconnect.
        If neither "reconnect" nor "redirect" is included in the instruction
        list, the client can continue with this server after completing all
        the instructions.
        """
        args = self._readlist()
        try:
            self.ui, lui = _loadnewui(self.ui, args)
        except error.ParseError as inst:
            dispatch._formatparse(self.ui.warn, inst)
            self.ui.flush()
            self.cresult.write('exit 255')
            return
        newhash = hashstate.fromui(lui, self.hashstate.mtimepaths)
        insts = []
        if newhash.mtimehash != self.hashstate.mtimehash:
            addr = _hashaddress(self.baseaddress, self.hashstate.confighash)
            insts.append('unlink %s' % addr)
            # mtimehash is empty if one or more extensions fail to load.
            # to be compatible with hg, still serve the client this time.
            if self.hashstate.mtimehash:
                insts.append('reconnect')
        if newhash.confighash != self.hashstate.confighash:
            addr = _hashaddress(self.baseaddress, newhash.confighash)
            insts.append('redirect %s' % addr)
        _log('validate: %s\n' % insts)
        self.cresult.write('\0'.join(insts) or '\0')

    def chdir(self):
        """Change current directory

        Note that the behavior of --cwd option is bit different from this.
        It does not affect --config parameter.
        """
        path = self._readstr()
        if not path:
            return
        _log('chdir to %r\n' % path)
        os.chdir(path)

    def setumask(self):
        """Change umask"""
        mask = struct.unpack('>I', self._read(4))[0]
        _log('setumask %r\n' % mask)
        os.umask(mask)

    def getpager(self):
        """Read cmdargs and write pager command to r-channel if enabled

        If pager isn't enabled, this writes '\0' because channeledoutput
        does not allow to write empty data.
        """
        args = self._readlist()
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
        l = self._readlist()
        try:
            newenv = dict(s.split('=', 1) for s in l)
        except ValueError:
            raise ValueError('unexpected value in setenv request')
        _log('setenv: %r\n' % sorted(newenv.keys()))
        os.environ.clear()
        os.environ.update(newenv)
        _clearenvaliases(commands.table)

    capabilities = commandserver.server.capabilities.copy()
    capabilities.update({'attachio': attachio,
                         'chdir': chdir,
                         'getpager': getpager,
                         'setenv': setenv,
                         'setumask': setumask})

# copied from mercurial/commandserver.py
class _requesthandler(SocketServer.StreamRequestHandler):
    def handle(self):
        # use a different process group from the master process, making this
        # process pass kernel "is_current_pgrp_orphaned" check so signals like
        # SIGTSTP, SIGTTIN, SIGTTOU are not ignored.
        os.setpgid(0, 0)
        # change random state otherwise forked request handlers would have a
        # same state inherited from parent.
        random.seed()
        ui = self.server.ui
        repo = self.server.repo
        sv = None
        try:
            sv = chgcmdserver(ui, repo, self.rfile, self.wfile, self.connection,
                              self.server.hashstate, self.server.baseaddress)
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
            if sv:
                cerr = sv.cerr
            else:
                cerr = commandserver.channeledoutput(self.wfile, 'e')
            traceback.print_exc(file=cerr)
            raise
        finally:
            # trigger __del__ since ForkingMixIn uses os._exit
            gc.collect()

def _tempaddress(address):
    return '%s.%d.tmp' % (address, os.getpid())

def _hashaddress(address, hashstr):
    return '%s-%s' % (address, hashstr)

class AutoExitMixIn:  # use old-style to comply with SocketServer design
    lastactive = time.time()
    idletimeout = 3600  # default 1 hour

    def startautoexitthread(self):
        # note: the auto-exit check here is cheap enough to not use a thread,
        # be done in serve_forever. however SocketServer is hook-unfriendly,
        # you simply cannot hook serve_forever without copying a lot of code.
        # besides, serve_forever's docstring suggests using thread.
        thread = threading.Thread(target=self._autoexitloop)
        thread.daemon = True
        thread.start()

    def _autoexitloop(self, interval=1):
        while True:
            time.sleep(interval)
            if not self.issocketowner():
                _log('%s is not owned, exiting.\n' % self.server_address)
                break
            if time.time() - self.lastactive > self.idletimeout:
                _log('being idle too long. exiting.\n')
                break
        self.shutdown()

    def process_request(self, request, address):
        self.lastactive = time.time()
        return SocketServer.ForkingMixIn.process_request(
            self, request, address)

    def server_bind(self):
        # use a unique temp address so we can stat the file and do ownership
        # check later
        tempaddress = _tempaddress(self.server_address)
        # use relative path instead of full path at bind() if possible, since
        # AF_UNIX path has very small length limit (107 chars) on common
        # platforms (see sys/un.h)
        dirname, basename = os.path.split(tempaddress)
        bakwdfd = None
        if dirname:
            bakwdfd = os.open('.', os.O_DIRECTORY)
            os.chdir(dirname)
        self.socket.bind(basename)
        self._socketstat = os.stat(basename)
        # rename will replace the old socket file if exists atomically. the
        # old server will detect ownership change and exit.
        util.rename(basename, self.server_address)
        if bakwdfd:
            os.fchdir(bakwdfd)
            os.close(bakwdfd)

    def issocketowner(self):
        try:
            stat = os.stat(self.server_address)
            return (stat.st_ino == self._socketstat.st_ino and
                    stat.st_mtime == self._socketstat.st_mtime)
        except OSError:
            return False

    def unlinksocketfile(self):
        if not self.issocketowner():
            return
        # it is possible to have a race condition here that we may
        # remove another server's socket file. but that's okay
        # since that server will detect and exit automatically and
        # the client will start a new server on demand.
        try:
            os.unlink(self.server_address)
        except OSError as exc:
            if exc.errno != errno.ENOENT:
                raise

class chgunixservice(commandserver.unixservice):
    def init(self):
        if self.repo:
            # one chgserver can serve multiple repos. drop repo infomation
            self.ui.setconfig('bundle', 'mainreporoot', '', 'repo')
            self.repo = None
        self._inithashstate()
        self._checkextensions()
        class cls(AutoExitMixIn, SocketServer.ForkingMixIn,
                  SocketServer.UnixStreamServer):
            ui = self.ui
            repo = self.repo
            hashstate = self.hashstate
            baseaddress = self.baseaddress
        self.server = cls(self.address, _requesthandler)
        self.server.idletimeout = self.ui.configint(
            'chgserver', 'idletimeout', self.server.idletimeout)
        self.server.startautoexitthread()
        self._createsymlink()

    def _inithashstate(self):
        self.baseaddress = self.address
        if self.ui.configbool('chgserver', 'skiphash', False):
            self.hashstate = None
            return
        self.hashstate = hashstate.fromui(self.ui)
        self.address = _hashaddress(self.address, self.hashstate.confighash)

    def _checkextensions(self):
        if not self.hashstate:
            return
        if extensions.notloaded():
            # one or more extensions failed to load. mtimehash becomes
            # meaningless because we do not know the paths of those extensions.
            # set mtimehash to an illegal hash value to invalidate the server.
            self.hashstate.mtimehash = ''

    def _createsymlink(self):
        if self.baseaddress == self.address:
            return
        tempaddress = _tempaddress(self.baseaddress)
        os.symlink(os.path.basename(self.address), tempaddress)
        util.rename(tempaddress, self.baseaddress)

    def run(self):
        try:
            self.server.serve_forever()
        finally:
            self.server.unlinksocketfile()

def uisetup(ui):
    commandserver._servicemap['chgunix'] = chgunixservice

    # CHGINTERNALMARK is temporarily set by chg client to detect if chg will
    # start another chg. drop it to avoid possible side effects.
    if 'CHGINTERNALMARK' in os.environ:
        del os.environ['CHGINTERNALMARK']
