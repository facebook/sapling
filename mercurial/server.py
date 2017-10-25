# server.py - utility and factory of server
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import tempfile

from .i18n import _

from . import (
    chgserver,
    cmdutil,
    commandserver,
    error,
    hgweb,
    pycompat,
    util,
)

def runservice(opts, parentfn=None, initfn=None, runfn=None, logfile=None,
               runargs=None, appendpid=False):
    '''Run a command as a service.'''

    def writepid(pid):
        if opts['pid_file']:
            if appendpid:
                mode = 'ab'
            else:
                mode = 'wb'
            fp = open(opts['pid_file'], mode)
            fp.write('%d\n' % pid)
            fp.close()

    if opts['daemon'] and not opts['daemon_postexec']:
        # Signal child process startup with file removal
        lockfd, lockpath = tempfile.mkstemp(prefix='hg-service-')
        os.close(lockfd)
        try:
            if not runargs:
                runargs = util.hgcmd() + pycompat.sysargv[1:]
            runargs.append('--daemon-postexec=unlink:%s' % lockpath)
            # Don't pass --cwd to the child process, because we've already
            # changed directory.
            for i in xrange(1, len(runargs)):
                if runargs[i].startswith('--cwd='):
                    del runargs[i]
                    break
                elif runargs[i].startswith('--cwd'):
                    del runargs[i:i + 2]
                    break
            def condfn():
                return not os.path.exists(lockpath)
            pid = util.rundetached(runargs, condfn)
            if pid < 0:
                raise error.Abort(_('child process failed to start'))
            writepid(pid)
        finally:
            util.tryunlink(lockpath)
        if parentfn:
            return parentfn(pid)
        else:
            return

    if initfn:
        initfn()

    if not opts['daemon']:
        writepid(util.getpid())

    if opts['daemon_postexec']:
        try:
            os.setsid()
        except AttributeError:
            pass
        for inst in opts['daemon_postexec']:
            if inst.startswith('unlink:'):
                lockpath = inst[7:]
                os.unlink(lockpath)
            elif inst.startswith('chdir:'):
                os.chdir(inst[6:])
            elif inst != 'none':
                raise error.Abort(_('invalid value for --daemon-postexec: %s')
                                  % inst)
        util.hidewindow()
        util.stdout.flush()
        util.stderr.flush()

        nullfd = os.open(os.devnull, os.O_RDWR)
        logfilefd = nullfd
        if logfile:
            logfilefd = os.open(logfile, os.O_RDWR | os.O_CREAT | os.O_APPEND,
                                0o666)
        os.dup2(nullfd, 0)
        os.dup2(logfilefd, 1)
        os.dup2(logfilefd, 2)
        if nullfd not in (0, 1, 2):
            os.close(nullfd)
        if logfile and logfilefd not in (0, 1, 2):
            os.close(logfilefd)

    if runfn:
        return runfn()

_cmdservicemap = {
    'chgunix': chgserver.chgunixservice,
    'pipe': commandserver.pipeservice,
    'unix': commandserver.unixforkingservice,
}

def _createcmdservice(ui, repo, opts):
    mode = opts['cmdserver']
    try:
        return _cmdservicemap[mode](ui, repo, opts)
    except KeyError:
        raise error.Abort(_('unknown mode %s') % mode)

def _createhgwebservice(ui, repo, opts):
    # this way we can check if something was given in the command-line
    if opts.get('port'):
        opts['port'] = util.getport(opts.get('port'))

    alluis = {ui}
    if repo:
        baseui = repo.baseui
        alluis.update([repo.baseui, repo.ui])
    else:
        baseui = ui
    webconf = opts.get('web_conf') or opts.get('webdir_conf')
    if webconf:
        if opts.get('subrepos'):
            raise error.Abort(_('--web-conf cannot be used with --subrepos'))

        # load server settings (e.g. web.port) to "copied" ui, which allows
        # hgwebdir to reload webconf cleanly
        servui = ui.copy()
        servui.readconfig(webconf, sections=['web'])
        alluis.add(servui)
    elif opts.get('subrepos'):
        servui = ui

        # If repo is None, hgweb.createapp() already raises a proper abort
        # message as long as webconf is None.
        if repo:
            webconf = dict()
            cmdutil.addwebdirpath(repo, "", webconf)
    else:
        servui = ui

    optlist = ("name templates style address port prefix ipv6"
               " accesslog errorlog certificate encoding")
    for o in optlist.split():
        val = opts.get(o, '')
        if val in (None, ''): # should check against default options instead
            continue
        for u in alluis:
            u.setconfig("web", o, val, 'serve')

    app = hgweb.createapp(baseui, repo, webconf)
    return hgweb.httpservice(servui, app, opts)

def createservice(ui, repo, opts):
    if opts["cmdserver"]:
        return _createcmdservice(ui, repo, opts)
    else:
        return _createhgwebservice(ui, repo, opts)
