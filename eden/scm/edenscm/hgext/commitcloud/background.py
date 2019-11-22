# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""background backup and sync

This module allows automatic backup or sync operations to be started after
every command that modifies the repo.

Automatic backups are enabled by setting the 'infinitepushbackup.autobackup'
config option to true.

Automatic backups can be temporarily disabled by setting
'infinitepushbackup.disableduntil' to a unix timestamp, or by running 'hg cloud
disable', which stores the disable time in the autobackup state file
('commitcloud/autobackup'). If both of these are set then backups are disabled
until both of them have expired.

The output from background backup or sync operations is logged to a directory
configured in the 'infinitepushbackup.logdir' config option.
"""

from __future__ import absolute_import

import errno
import json
import os
import stat
import subprocess
import time

from edenscm.mercurial import (
    dispatch,
    encoding,
    error,
    extensions,
    localrepo,
    policy,
    util,
)
from edenscm.mercurial.i18n import _

from . import util as ccutil, workspace


def extsetup(ui):
    extensions.wrapfunction(dispatch, "runcommand", _runcommand)
    extensions.wrapfunction(localrepo.localrepository, "transaction", _transaction)


# Autobackup state
#
# The autobackupstatefile contains a JSON object containing state for
# commitcloud automatic backups.
#
# Valid fields are:
#
# "disableduntil" - An integer unixtime that automatic backup is disabled until.
_autobackupstatefile = "commitcloud/autobackup"


def loadautobackupstate(repo):
    try:
        with repo.sharedvfs.open(_autobackupstatefile) as f:
            return json.load(f)
    except IOError as e:
        if e.errno != errno.ENOENT:
            raise
    except Exception:
        repo.ui.warn(_("invalid commitcloud autobackup state - ignoring\n"))
    return {}


def saveautobackupstate(repo, state):
    repo.sharedvfs.makedirs("commitcloud")
    with repo.sharedvfs.open(_autobackupstatefile, "w", atomictemp=True) as f:
        json.dump(state, f)


def disableautobackup(repo, until):
    state = loadautobackupstate(repo)
    if until is not None:
        state["disableduntil"] = until
    else:
        state.pop("disableduntil", None)
    saveautobackupstate(repo, state)


def autobackupdisableduntil(repo):
    """returns the timestamp that backup disable expires at

    Backup can be disabled by the user, either in config, or by running
    'hg cloud disable', which stores its state in the autobackup state.
    """
    # developer config: infinitepushbackup.disableduntil
    return max(
        repo.ui.configint("infinitepushbackup", "disableduntil", None),
        util.parseint(loadautobackupstate(repo).get("disableduntil")),
    )


def autobackupenabled(repo):
    # Backup is possibly disabled by user, but the disabling might have expired.
    # developer config: infinitepushbackup.disableduntil
    timestamp = autobackupdisableduntil(repo)
    if timestamp is not None and time.time() <= timestamp:
        return False
    return repo.ui.configbool("infinitepushbackup", "autobackup")


def _runcommand(orig, lui, repo, cmd, fullargs, *args):
    """start an automatic backup or cloud sync after every command

    Since we don't want to start auto backup after read-only commands,
    then this wrapper checks if this command opened at least one transaction.
    If yes then background backup will be started.
    """
    try:
        return orig(lui, repo, cmd, fullargs, *args)
    finally:
        # For chg, do not wrap the "serve" runcommand call.  Otherwise, if
        # autobackup is enabled for the repo, and a transaction was opened
        # to modify the repo, start an automatic background backup.
        if (
            "CHGINTERNALMARK" not in encoding.environ
            and repo is not None
            and autobackupenabled(repo)
            and getattr(repo, "txnwasopened", False)
            and not getattr(repo, "ignoreautobackup", False)
        ):
            lui.debug("starting commit cloud autobackup in the background\n")
            backgroundbackup(repo)


def _transaction(orig, self, *args, **kwargs):
    """record if a transaction was opened

    If a transaction was opened then we want to start a background backup or
    cloud sync.  Record the fact that transaction was opened.
    """
    self.txnwasopened = True
    return orig(self, *args, **kwargs)


def backgroundbackupother(repo, dest=None):
    """start background backup for the other remote

    Commit cloud can be configured to back up to a secondary backup server by
    setting the 'infinitepush-other' remote to the path to the secondary server.
    If this is set, and it differs from the main 'infinitepush' remote, start
    background backup for the other remote.
    """
    other = "infinitepush-other"
    try:
        remotepath = repo.ui.paths.getpath(other)
    except error.RepoError:
        remotepath = None
    if remotepath and remotepath.loc != ccutil.getremotepath(repo, dest):
        repo.ui.debug("starting background backup to %s\n" % remotepath.loc)
        backgroundbackup(repo, ["hg", "cloud", "backup"], dest=other)


def backgroundbackup(repo, command=None, dest=None):
    """start background backup"""
    ui = repo.ui
    if command is not None:
        background_cmd = command
    elif workspace.currentworkspace(repo):
        background_cmd = ["hg", "cloud", "sync"]
    else:
        background_cmd = ["hg", "cloud", "backup"]
    infinitepush_bgssh = ui.config("infinitepush", "bgssh")
    if infinitepush_bgssh:
        background_cmd += ["--config", "ui.ssh=%s" % infinitepush_bgssh]

    # developer config: infinitepushbackup.bgdebuglocks
    if ui.configbool("infinitepushbackup", "bgdebuglocks"):
        background_cmd += ["--config", "devel.debug-lockers=true"]

    # developer config: infinitepushbackup.bgdebug
    if ui.configbool("infinitepushbackup", "bgdebug", False):
        background_cmd.append("--debug")

    if dest:
        background_cmd += ["--dest", dest]

    logfile = None
    logdir = ui.config("infinitepushbackup", "logdir")
    if logdir:
        # make newly created files and dirs non-writable
        oldumask = os.umask(0o022)
        try:
            try:
                # the user name from the machine
                username = util.getuser()
            except Exception:
                username = "unknown"

            if not _checkcommonlogdir(logdir):
                raise WrongPermissionsException(logdir)

            userlogdir = os.path.join(logdir, username)
            util.makedirs(userlogdir)

            if not _checkuserlogdir(userlogdir):
                raise WrongPermissionsException(userlogdir)

            reponame = os.path.basename(repo.sharedroot)
            _removeoldlogfiles(userlogdir, reponame)
            logfile = _getlogfilename(logdir, username, reponame)
        except (OSError, IOError) as e:
            ui.debug("background backup log is disabled: %s\n" % e)
        except WrongPermissionsException as e:
            ui.debug(
                (
                    "%s directory has incorrect permission, "
                    + "background backup logging will be disabled\n"
                )
                % e.logdir
            )
        finally:
            os.umask(oldumask)

    if not logfile:
        logfile = os.devnull

    with open(logfile, "a") as f, open(os.devnull, "r") as fin:
        timestamp = util.datestr(util.makedate(), "%Y-%m-%d %H:%M:%S %z")
        fullcmd = " ".join(util.shellquote(arg) for arg in background_cmd)
        f.write("\n%s starting: %s\n" % (timestamp, fullcmd))
        subprocess.Popen(
            background_cmd, shell=False, stdin=fin, stdout=f, stderr=subprocess.STDOUT
        )


class WrongPermissionsException(Exception):
    def __init__(self, logdir):
        self.logdir = logdir


_timeformat = "%Y%m%d"


def _getlogfilename(logdir, username, reponame):
    """Returns name of the log file for particular user and repo

    Different users have different directories inside logdir. Log filename
    consists of reponame (basename of repo path) and current day
    (see _timeformat). That means that two different repos with the same name
    can share the same log file. This is not a big problem so we ignore it.
    """

    currentday = time.strftime(_timeformat)
    return os.path.join(logdir, username, reponame + currentday)


def _removeoldlogfiles(userlogdir, reponame):
    existinglogfiles = []
    for entry in util.listdir(userlogdir):
        filename = entry[0]
        fullpath = os.path.join(userlogdir, filename)
        if filename.startswith(reponame) and os.path.isfile(fullpath):
            try:
                time.strptime(filename[len(reponame) :], _timeformat)
            except ValueError:
                continue
            existinglogfiles.append(filename)

    # _timeformat gives us a property that if we sort log file names in
    # descending order then newer files are going to be in the beginning
    existinglogfiles = sorted(existinglogfiles, reverse=True)
    # Delete logs that are older than 5 days
    maxlogfilenumber = 5
    if len(existinglogfiles) > maxlogfilenumber:
        for filename in existinglogfiles[maxlogfilenumber:]:
            os.unlink(os.path.join(userlogdir, filename))


def _checkcommonlogdir(logdir):
    """Checks permissions of the log directory

    We want log directory to actually be a directory, have restricting
    deletion flag set (sticky bit)
    """

    try:
        st = os.stat(logdir)
        return stat.S_ISDIR(st.st_mode) and st.st_mode & stat.S_ISVTX
    except OSError:
        # is raised by os.stat()
        return False


def _checkuserlogdir(userlogdir):
    """Checks permissions of the user log directory

    We want user log directory to be writable only by the user who created it
    and be owned by `username`
    """

    try:
        st = os.stat(userlogdir)
        # Check that `userlogdir` is owned by `username`
        if os.getuid() != st.st_uid:
            return False
        return (
            st.st_mode & (stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH)
        ) == stat.S_IWUSR
    except OSError:
        # is raised by os.stat()
        return False
