# sshpeer.py - ssh repository proxy class for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import re
from i18n import _
import util, error, wireproto

class remotelock(object):
    def __init__(self, repo):
        self.repo = repo
    def release(self):
        self.repo.unlock()
        self.repo = None
    def __del__(self):
        if self.repo:
            self.release()

def _serverquote(s):
    if not s:
        return s
    '''quote a string for the remote shell ... which we assume is sh'''
    if re.match('[a-zA-Z0-9@%_+=:,./-]*$', s):
        return s
    return "'%s'" % s.replace("'", "'\\''")

def _forwardoutput(ui, pipe):
    """display all data currently available on pipe as remote output.

    This is non blocking."""
    s = util.readpipe(pipe)
    if s:
        for l in s.splitlines():
            ui.status(_("remote: "), l, '\n')

class doublepipe(object):
    """Operate a side-channel pipe in addition of a main one

    The side-channel pipe contains server output to be forwarded to the user
    input. The double pipe will behave as the "main" pipe, but will ensure the
    content of the "side" pipe is properly processed while we wait for blocking
    call on the "main" pipe.

    If large amounts of data are read from "main", the forward will cease after
    the first bytes start to appear. This simplifies the implementation
    without affecting actual output of sshpeer too much as we rarely issue
    large read for data not yet emitted by the server.

    The main pipe is expected to be a 'bufferedinputpipe' from the util module
    that handle all the os specific bites. This class lives in this module
    because it focus on behavior specifig to the ssh protocol."""

    def __init__(self, ui, main, side):
        self._ui = ui
        self._main = main
        self._side = side

    def _wait(self):
        """wait until some data are available on main or side

        return a pair of boolean (ismainready, issideready)

        (This will only wait for data if the setup is supported by `util.poll`)
        """
        if getattr(self._main, 'hasbuffer', False): # getattr for classic pipe
            return (True, True) # main has data, assume side is worth poking at.
        fds = [self._main.fileno(), self._side.fileno()]
        try:
            act = util.poll(fds)
        except NotImplementedError:
            # non supported yet case, assume all have data.
            act = fds
        return (self._main.fileno() in act, self._side.fileno() in act)

    def write(self, data):
        return self._call('write', data)

    def read(self, size):
        return self._call('read', size)

    def readline(self):
        return self._call('readline')

    def _call(self, methname, data=None):
        """call <methname> on "main", forward output of "side" while blocking
        """
        # data can be '' or 0
        if (data is not None and not data) or self._main.closed:
            _forwardoutput(self._ui, self._side)
            return ''
        while True:
            mainready, sideready = self._wait()
            if sideready:
                _forwardoutput(self._ui, self._side)
            if mainready:
                meth = getattr(self._main, methname)
                if data is None:
                    return meth()
                else:
                    return meth(data)

    def close(self):
        return self._main.close()

    def flush(self):
        return self._main.flush()

class sshpeer(wireproto.wirepeer):
    def __init__(self, ui, path, create=False):
        self._url = path
        self.ui = ui
        self.pipeo = self.pipei = self.pipee = None

        u = util.url(path, parsequery=False, parsefragment=False)
        if u.scheme != 'ssh' or not u.host or u.path is None:
            self._abort(error.RepoError(_("couldn't parse location %s") % path))

        self.user = u.user
        if u.passwd is not None:
            self._abort(error.RepoError(_("password in URL not supported")))
        self.host = u.host
        self.port = u.port
        self.path = u.path or "."

        sshcmd = self.ui.config("ui", "ssh", "ssh")
        remotecmd = self.ui.config("ui", "remotecmd", "hg")

        args = util.sshargs(sshcmd,
                            _serverquote(self.host),
                            _serverquote(self.user),
                            _serverquote(self.port))

        if create:
            cmd = '%s %s %s' % (sshcmd, args,
                util.shellquote("%s init %s" %
                    (_serverquote(remotecmd), _serverquote(self.path))))
            ui.debug('running %s\n' % cmd)
            res = ui.system(cmd)
            if res != 0:
                self._abort(error.RepoError(_("could not create remote repo")))

        self._validaterepo(sshcmd, args, remotecmd)

    def url(self):
        return self._url

    def _validaterepo(self, sshcmd, args, remotecmd):
        # cleanup up previous run
        self.cleanup()

        cmd = '%s %s %s' % (sshcmd, args,
            util.shellquote("%s -R %s serve --stdio" %
                (_serverquote(remotecmd), _serverquote(self.path))))
        self.ui.debug('running %s\n' % cmd)
        cmd = util.quotecommand(cmd)

        # while self.subprocess isn't used, having it allows the subprocess to
        # to clean up correctly later
        #
        # no buffer allow the use of 'select'
        # feel free to remove buffering and select usage when we ultimately
        # move to threading.
        sub = util.popen4(cmd, bufsize=0)
        self.pipeo, self.pipei, self.pipee, self.subprocess = sub

        self.pipei = util.bufferedinputpipe(self.pipei)
        self.pipei = doublepipe(self.ui, self.pipei, self.pipee)
        self.pipeo = doublepipe(self.ui, self.pipeo, self.pipee)

        # skip any noise generated by remote shell
        self._callstream("hello")
        r = self._callstream("between", pairs=("%s-%s" % ("0"*40, "0"*40)))
        lines = ["", "dummy"]
        max_noise = 500
        while lines[-1] and max_noise:
            l = r.readline()
            self.readerr()
            if lines[-1] == "1\n" and l == "\n":
                break
            if l:
                self.ui.debug("remote: ", l)
            lines.append(l)
            max_noise -= 1
        else:
            self._abort(error.RepoError(_('no suitable response from '
                                          'remote hg')))

        self._caps = set()
        for l in reversed(lines):
            if l.startswith("capabilities:"):
                self._caps.update(l[:-1].split(":")[1].split())
                break

    def _capabilities(self):
        return self._caps

    def readerr(self):
        _forwardoutput(self.ui, self.pipee)

    def _abort(self, exception):
        self.cleanup()
        raise exception

    def cleanup(self):
        if self.pipeo is None:
            return
        self.pipeo.close()
        self.pipei.close()
        try:
            # read the error descriptor until EOF
            for l in self.pipee:
                self.ui.status(_("remote: "), l)
        except (IOError, ValueError):
            pass
        self.pipee.close()

    __del__ = cleanup

    def _callstream(self, cmd, **args):
        self.ui.debug("sending %s command\n" % cmd)
        self.pipeo.write("%s\n" % cmd)
        _func, names = wireproto.commands[cmd]
        keys = names.split()
        wireargs = {}
        for k in keys:
            if k == '*':
                wireargs['*'] = args
                break
            else:
                wireargs[k] = args[k]
                del args[k]
        for k, v in sorted(wireargs.iteritems()):
            self.pipeo.write("%s %d\n" % (k, len(v)))
            if isinstance(v, dict):
                for dk, dv in v.iteritems():
                    self.pipeo.write("%s %d\n" % (dk, len(dv)))
                    self.pipeo.write(dv)
            else:
                self.pipeo.write(v)
        self.pipeo.flush()

        return self.pipei

    def _callcompressable(self, cmd, **args):
        return self._callstream(cmd, **args)

    def _call(self, cmd, **args):
        self._callstream(cmd, **args)
        return self._recv()

    def _callpush(self, cmd, fp, **args):
        r = self._call(cmd, **args)
        if r:
            return '', r
        while True:
            d = fp.read(4096)
            if not d:
                break
            self._send(d)
        self._send("", flush=True)
        r = self._recv()
        if r:
            return '', r
        return self._recv(), ''

    def _calltwowaystream(self, cmd, fp, **args):
        r = self._call(cmd, **args)
        if r:
            # XXX needs to be made better
            raise util.Abort('unexpected remote reply: %s' % r)
        while True:
            d = fp.read(4096)
            if not d:
                break
            self._send(d)
        self._send("", flush=True)
        return self.pipei

    def _recv(self):
        l = self.pipei.readline()
        if l == '\n':
            self.readerr()
            msg = _('check previous remote output')
            self._abort(error.OutOfBandError(hint=msg))
        self.readerr()
        try:
            l = int(l)
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), l))
        return self.pipei.read(l)

    def _send(self, data, flush=False):
        self.pipeo.write("%d\n" % len(data))
        if data:
            self.pipeo.write(data)
        if flush:
            self.pipeo.flush()
        self.readerr()

    def lock(self):
        self._call("lock")
        return remotelock(self)

    def unlock(self):
        self._call("unlock")

    def addchangegroup(self, cg, source, url, lock=None):
        '''Send a changegroup to the remote server.  Return an integer
        similar to unbundle(). DEPRECATED, since it requires locking the
        remote.'''
        d = self._call("addchangegroup")
        if d:
            self._abort(error.RepoError(_("push refused: %s") % d))
        while True:
            d = cg.read(4096)
            if not d:
                break
            self.pipeo.write(d)
            self.readerr()

        self.pipeo.flush()

        self.readerr()
        r = self._recv()
        if not r:
            return 1
        try:
            return int(r)
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), r))

instance = sshpeer
