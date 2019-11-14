# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import contextlib
import errno
import json
import subprocess

from edenscm.mercurial import error, lock as lockmod, node as nodemod, pycompat, util
from edenscm.mercurial.i18n import _


lockfilename = "infinitepushbackup.lock"

# Progress reporting
# Progress reports for ongoing backups and syncs are written to a file
# (protected by the backup lock). This file contains JSON format data of the
# form:
# {
#    "step": "description of the current step",
#    "data": { ... }
# }
# The "data" dict may contain:
#    "newheads", a list of commit hashes of the heads of new commits that are
#    being pulled into the repo
#    "backingup", a list of commit hashes of commits that are being backed up
progressfilename = "commitcloudsyncprogress"


def progress(repo, step, **kwargs):
    with repo.sharedvfs.open(progressfilename, "w", atomictemp=True) as f:
        data = {"step": str(step), "data": kwargs}
        json.dump(data, f)


def progressbackingup(repo, nodes):
    if len(nodes) == 1:
        msg = "backing up %s" % nodemod.short(nodes[0])
    else:
        msg = "backing up %d commits" % len(nodes)
    hexnodes = [nodemod.hex(node) for node in nodes]
    progress(repo, msg, backingup=hexnodes)


def progresspulling(repo, heads):
    if len(heads) == 1:
        msg = "pulling %s" % nodemod.short(heads[0])
    else:
        msg = "pulling %d new heads" % len(heads)
    hexheads = [nodemod.hex(head) for head in heads]
    progress(repo, msg, pulling=hexheads)


def progresscomplete(repo):
    repo.sharedvfs.tryunlink(progressfilename)


def _getprogressstep(repo):
    try:
        data = json.load(repo.sharedvfs.open(progressfilename))
    except IOError as e:
        if e.errno != errno.ENOENT:
            raise
    else:
        return data.get("step")


def _getprocessetime(locker):
    """return etime in seconds for the process that is
     holding the lock
    """
    # TODO: support windows
    if not pycompat.isposix:
        return None
    if not locker.pid or not locker.issamenamespace():
        return None
    try:
        pid = locker.pid
        p = subprocess.Popen(
            ["ps", "-o", "etime=", pid],
            stdin=None,
            close_fds=util.closefds,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        (stdoutdata, stderrdata) = p.communicate()
        if p.returncode == 0 and stdoutdata:
            etime = stdoutdata.strip()
            # `ps` format for etime is [[dd-]hh:]mm:s
            # examples:
            #     '21-18:26:30',
            #     '06-00:15:30',
            #     '15:28:37',
            #      '48:14',
            #      '00:01'
            splits = etime.replace("-", ":").split(":")
            t = [int(i) for i in reversed(splits)] + [0] * (4 - len(splits))
            etimesec = t[3] * 86400 + t[2] * 3600 + t[1] * 60 + t[0]
            return etimesec
    except Exception:
        return None


@contextlib.contextmanager
def trylock(repo):
    try:
        with lockmod.trylock(repo.ui, repo.sharedvfs, lockfilename, 0, 0) as lock:
            yield lock
    except error.LockHeld as e:
        if e.lockinfo.isrunning():
            lockinfo = e.lockinfo
            etime = _getprocessetime(lockinfo)
            if etime:
                minutes, seconds = divmod(etime, 60)
                etimemsg = _("\n(pid %s on %s, running for %d min %d sec)") % (
                    lockinfo.uniqueid,
                    lockinfo.namespace,
                    minutes,
                    seconds,
                )
            else:
                etimemsg = ""
            bgstep = _getprogressstep(repo) or "synchronizing"
            repo.ui.status(
                _("background cloud sync is in progress: %s%s\n") % (bgstep, etimemsg),
                component="commitcloud",
            )
        raise


@contextlib.contextmanager
def lock(repo):
    # First speculatively try to lock so that we immediately print info about
    # the lock if it is locked.
    if repo.ui.interactive():
        try:
            with trylock(repo) as lock:
                yield lock
                return
        except error.LockHeld:
            pass

    # Now just wait for the lock.  Wait up to 120 seconds, because cloud sync
    # can take a while.
    with lockmod.lock(
        repo.sharedvfs,
        lockfilename,
        timeout=120,
        ui=repo.ui,
        showspinner=True,
        spinnermsg=_("waiting for background process to complete"),
    ) as lock:
        yield lock


def islocked(repo):
    return util.islocked(repo.sharedvfs.join(lockfilename))


def status(repo):
    try:
        with trylock(repo):
            pass
    except error.LockHeld:
        pass
