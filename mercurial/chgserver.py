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

from .i18n import _

from . import (
    commandserver,
    encoding,
    error,
    extensions,
    pycompat,
    util,
)

_log = commandserver.log

def _hashlist(items):
    """return sha1 hexdigest for a list"""
    return hashlib.sha1(str(items)).hexdigest()

# sensitive config sections affecting confighash
_configsections = [
    'alias',  # affects global state commands.table
    'eol',    # uses setconfig('eol', ...)
    'extdiff',  # uisetup will register new commands
    'extensions',
]

_configsectionitems = [
    ('commands', 'show.aliasprefix'), # show.py reads it in extsetup
]

# sensitive environment variables affecting confighash
_envre = re.compile(r'''\A(?:
                    CHGHG
                    |HG(?:DEMANDIMPORT|EMITWARNINGS|MODULEPOLICY|PROF|RCPATH)?
                    |HG(?:ENCODING|PLAIN).*
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
    for section, item in _configsectionitems:
        sectionitems.append(ui.config(section, item))
    sectionhash = _hashlist(sectionitems)
    # If $CHGHG is set, the change to $HG should not trigger a new chg server
    if 'CHGHG' in encoding.environ:
        ignored = {'HG'}
    else:
        ignored = set()
    envitems = [(k, v) for k, v in encoding.environ.iteritems()
                if _envre.match(k) and k not in ignored]
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
        from . import __version__
        modules.append(__version__)
    except ImportError:
        pass
    files = [pycompat.sysexecutable]
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

    mtimehash is not included in confighash because we only know the paths of
    extensions after importing them (there is imp.find_module but that faces
    race conditions). We need to calculate confighash without importing.
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

def _newchgui(srcui, csystem, attachio):
    class chgui(srcui.__class__):
        def __init__(self, src=None):
            super(chgui, self).__init__(src)
            if src:
                self._csystem = getattr(src, '_csystem', csystem)
            else:
                self._csystem = csystem

        def _runsystem(self, cmd, environ, cwd, out):
            # fallback to the original system method if the output needs to be
            # captured (to self._buffers), or the output stream is not stdout
            # (e.g. stderr, cStringIO), because the chg client is not aware of
            # these situations and will behave differently (write to stdout).
            if (out is not self.fout
                or not util.safehasattr(self.fout, 'fileno')
                or self.fout.fileno() != util.stdout.fileno()):
                return util.system(cmd, environ=environ, cwd=cwd, out=out)
            self.flush()
            return self._csystem(cmd, util.shellenviron(environ), cwd)

        def _runpager(self, cmd, env=None):
            self._csystem(cmd, util.shellenviron(env), type='pager',
                          cmdtable={'attachio': attachio})
            return True

    return chgui(srcui)

def _loadnewui(srcui, args):
    from . import dispatch  # avoid cycle

    newui = srcui.__class__.load()
    for a in ['fin', 'fout', 'ferr', 'environ']:
        setattr(newui, a, getattr(srcui, a))
    if util.safehasattr(srcui, '_csystem'):
        newui._csystem = srcui._csystem

    # command line args
    options = {}
    if srcui.plain('strictflags'):
        options.update(dispatch._earlyparseopts(args))
    else:
        args = args[:]
        options['config'] = dispatch._earlygetopt(['--config'], args)
        cwds = dispatch._earlygetopt(['--cwd'], args)
        options['cwd'] = cwds and cwds[-1] or ''
        rpath = dispatch._earlygetopt(["-R", "--repository", "--repo"], args)
        options['repository'] = rpath and rpath[-1] or ''
    dispatch._parseconfig(newui, options['config'])

    # stolen from tortoisehg.util.copydynamicconfig()
    for section, name, value in srcui.walkconfig():
        source = srcui.configsource(section, name)
        if ':' in source or source == '--config' or source.startswith('$'):
            # path:line or command line, or environ
            continue
        newui.setconfig(section, name, value, source)

    # load wd and repo config, copied from dispatch.py
    cwd = options['cwd']
    cwd = cwd and os.path.realpath(cwd) or None
    rpath = options['repository']
    path, newlui = dispatch._getlocal(newui, rpath, wd=cwd)

    return (newui, newlui)

class channeledsystem(object):
    """Propagate ui.system() request in the following format:

    payload length (unsigned int),
    type, '\0',
    cmd, '\0',
    cwd, '\0',
    envkey, '=', val, '\0',
    ...
    envkey, '=', val

    if type == 'system', waits for:

    exitcode length (unsigned int),
    exitcode (int)

    if type == 'pager', repetitively waits for a command name ending with '\n'
    and executes it defined by cmdtable, or exits the loop if the command name
    is empty.
    """
    def __init__(self, in_, out, channel):
        self.in_ = in_
        self.out = out
        self.channel = channel

    def __call__(self, cmd, environ, cwd=None, type='system', cmdtable=None):
        args = [type, util.quotecommand(cmd), os.path.abspath(cwd or '.')]
        args.extend('%s=%s' % (k, v) for k, v in environ.iteritems())
        data = '\0'.join(args)
        self.out.write(struct.pack('>cI', self.channel, len(data)))
        self.out.write(data)
        self.out.flush()

        if type == 'system':
            length = self.in_.read(4)
            length, = struct.unpack('>I', length)
            if length != 4:
                raise error.Abort(_('invalid response'))
            rc, = struct.unpack('>i', self.in_.read(4))
            return rc
        elif type == 'pager':
            while True:
                cmd = self.in_.readline()[:-1]
                if not cmd:
                    break
                if cmdtable and cmd in cmdtable:
                    _log('pager subcommand: %s' % cmd)
                    cmdtable[cmd]()
                else:
                    raise error.Abort(_('unexpected command: %s') % cmd)
        else:
            raise error.ProgrammingError('invalid S channel type: %s' % type)

_iochannels = [
    # server.ch, ui.fp, mode
    ('cin', 'fin', pycompat.sysstr('rb')),
    ('cout', 'fout', pycompat.sysstr('wb')),
    ('cerr', 'ferr', pycompat.sysstr('wb')),
]

class chgcmdserver(commandserver.server):
    def __init__(self, ui, repo, fin, fout, sock, hashstate, baseaddress):
        super(chgcmdserver, self).__init__(
            _newchgui(ui, channeledsystem(fin, fout, 'S'), self.attachio),
            repo, fin, fout)
        self.clientsock = sock
        self._oldios = []  # original (self.ch, ui.fp, fd) before "attachio"
        self.hashstate = hashstate
        self.baseaddress = baseaddress
        if hashstate is not None:
            self.capabilities = self.capabilities.copy()
            self.capabilities['validate'] = chgcmdserver.validate

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
        self.clientsock.sendall(struct.pack('>cI', 'I', 1))
        clientfds = util.recvfds(self.clientsock.fileno())
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
        from . import dispatch  # avoid cycle

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

    def runcommand(self):
        return super(chgcmdserver, self).runcommand()

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
        encoding.environ.clear()
        encoding.environ.update(newenv)

    capabilities = commandserver.server.capabilities.copy()
    capabilities.update({'attachio': attachio,
                         'chdir': chdir,
                         'runcommand': runcommand,
                         'setenv': setenv,
                         'setumask': setumask})

    if util.safehasattr(util, 'setprocname'):
        def setprocname(self):
            """Change process title"""
            name = self._readstr()
            _log('setprocname: %r\n' % name)
            util.setprocname(name)
        capabilities['setprocname'] = setprocname

def _tempaddress(address):
    return '%s.%d.tmp' % (address, os.getpid())

def _hashaddress(address, hashstr):
    # if the basename of address contains '.', use only the left part. this
    # makes it possible for the client to pass 'server.tmp$PID' and follow by
    # an atomic rename to avoid locking when spawning new servers.
    dirname, basename = os.path.split(address)
    basename = basename.split('.', 1)[0]
    return '%s-%s' % (os.path.join(dirname, basename), hashstr)

class chgunixservicehandler(object):
    """Set of operations for chg services"""

    pollinterval = 1  # [sec]

    def __init__(self, ui):
        self.ui = ui
        self._idletimeout = ui.configint('chgserver', 'idletimeout')
        self._lastactive = time.time()

    def bindsocket(self, sock, address):
        self._inithashstate(address)
        self._checkextensions()
        self._bind(sock)
        self._createsymlink()
        # no "listening at" message should be printed to simulate hg behavior

    def _inithashstate(self, address):
        self._baseaddress = address
        if self.ui.configbool('chgserver', 'skiphash'):
            self._hashstate = None
            self._realaddress = address
            return
        self._hashstate = hashstate.fromui(self.ui)
        self._realaddress = _hashaddress(address, self._hashstate.confighash)

    def _checkextensions(self):
        if not self._hashstate:
            return
        if extensions.notloaded():
            # one or more extensions failed to load. mtimehash becomes
            # meaningless because we do not know the paths of those extensions.
            # set mtimehash to an illegal hash value to invalidate the server.
            self._hashstate.mtimehash = ''

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
            return (stat.st_ino == self._socketstat.st_ino and
                    stat.st_mtime == self._socketstat.st_mtime)
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
            self.ui.debug('%s is not owned, exiting.\n' % self._realaddress)
            return True
        if time.time() - self._lastactive > self._idletimeout:
            self.ui.debug('being idle too long. exiting.\n')
            return True
        return False

    def newconnection(self):
        self._lastactive = time.time()

    def createcmdserver(self, repo, conn, fin, fout):
        return chgcmdserver(self.ui, repo, fin, fout, conn,
                            self._hashstate, self._baseaddress)

def chgunixservice(ui, repo, opts):
    # CHGINTERNALMARK is set by chg client. It is an indication of things are
    # started by chg so other code can do things accordingly, like disabling
    # demandimport or detecting chg client started by chg client. When executed
    # here, CHGINTERNALMARK is no longer useful and hence dropped to make
    # environ cleaner.
    if 'CHGINTERNALMARK' in encoding.environ:
        del encoding.environ['CHGINTERNALMARK']

    if repo:
        # one chgserver can serve multiple repos. drop repo information
        ui.setconfig('bundle', 'mainreporoot', '', 'repo')
    h = chgunixservicehandler(ui)
    return commandserver.unixforkingservice(ui, repo=None, opts=opts, handler=h)
