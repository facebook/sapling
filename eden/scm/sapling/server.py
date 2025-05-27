# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# server.py - utility and factory of server
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import os
import sys
import tempfile

from . import chgserver, commandserver, error, util
from .i18n import _


def runservice(
    opts,
    parentfn=None,
    initfn=None,
    runfn=None,
    logfile=None,
    runargs=None,
    appendpid=False,
):
    """Run a command as a service."""

    def writepid(pid):
        if opts["pid_file"]:
            if appendpid:
                mode = "ab"
            else:
                mode = "wb"
            fp = open(opts["pid_file"], mode)
            fp.write(b"%d\n" % pid)
            fp.close()

    if opts["daemon"] and not opts["daemon_postexec"]:
        # Signal child process startup with file removal
        lockfd, lockpath = tempfile.mkstemp(prefix="hg-service-")
        os.close(lockfd)
        portpath = opts.get("port_file")
        if portpath:
            util.tryunlink(portpath)
        try:
            if not runargs:
                runargs = util.hgcmd() + sys.argv[1:]
            runargs.append("--daemon-postexec=unlink:%s" % lockpath)
            # Don't pass --cwd to the child process, because we've already
            # changed directory.
            for i in range(1, len(runargs)):
                if runargs[i].startswith("--cwd="):
                    del runargs[i]
                    break
                elif runargs[i].startswith("--cwd"):
                    del runargs[i : i + 2]
                    break

            def condfn():
                if portpath and not os.path.exists(portpath):
                    return False
                return not os.path.exists(lockpath)

            pid = util.rundetached(runargs, condfn)
            if pid < 0:
                raise error.Abort(_("child process failed to start"))
            writepid(pid)
        finally:
            util.tryunlink(lockpath)
        if parentfn:
            return parentfn(pid)
        else:
            return

    if initfn:
        initfn()

    if not opts["daemon"]:
        writepid(util.getpid())

    if opts["daemon_postexec"]:
        try:
            os.setsid()
        except (AttributeError, OSError):
            # OSError can happen if spawn-ext already does setsid().
            pass
        for inst in opts["daemon_postexec"]:
            if inst.startswith("unlink:"):
                lockpath = inst[7:]
                os.unlink(lockpath)
            elif inst.startswith("chdir:"):
                os.chdir(inst[6:])
            elif inst != "none":
                raise error.Abort(_("invalid value for --daemon-postexec: %s") % inst)
        util.hidewindow()
        util.stdout.flush()
        util.stderr.flush()

        nullfd = os.open(os.devnull, os.O_RDWR)
        logfilefd = nullfd
        if logfile:
            logfilefd = os.open(logfile, os.O_RDWR | os.O_CREAT | os.O_APPEND, 0o666)
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
    "chgunix2": chgserver.chgunixservice,
    "pipe": commandserver.pipeservice,
    "unix": commandserver.unixforkingservice,
}


def _createcmdservice(ui, repo, opts):
    mode = opts["cmdserver"]
    try:
        return _cmdservicemap[mode](ui, repo, opts)
    except KeyError:
        raise error.Abort(_("unknown mode %s") % mode)


def createservice(ui, repo, opts):
    if opts["cmdserver"]:
        return _createcmdservice(ui, repo, opts)
    else:
        raise error.Abort(_("web server no longer supported"))
