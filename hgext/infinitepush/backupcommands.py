# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
    [infinitepushbackup]
    # Whether to enable automatic backups. If this option is True then a backup
    # process will be started after every mercurial command that modifies the
    # repo, for example, commit, amend, histedit, rebase etc.
    autobackup = False

    # path to the directory where pushback logs should be stored
    logdir = path/to/dir

    # Backup at most maxheadstobackup heads, other heads are ignored.
    # Negative number means backup everything.
    maxheadstobackup = -1

    # Nodes that should not be backed up. Ancestors of these nodes won't be
    # backed up either
    dontbackupnodes = []

    # Special option that may be used to trigger re-backuping. For example,
    # if there was a bug in infinitepush backups, then changing the value of
    # this option will force all clients to make a "clean" backup
    backupgeneration = 0

    # Hostname value to use. If not specified then socket.gethostname() will
    # be used
    hostname = ''

    # Enable reporting of background backup status as a summary at the end
    # of smartlog.
    enablestatus = False

    # Whether or not to save information about the latest successful backup.
    # This information includes the local revision number and unix timestamp
    # of the last time we successfully made a backup.
    savelatestbackupinfo = False

    # Enable creating obsolete markers when backup is restored.
    createlandedasmarkers = False

    # Number of backups to list by default in getavailablebackups
    backuplistlimit = 10
"""
from __future__ import absolute_import

import collections
import ConfigParser
import errno
import json
import os
import re
import socket
import stat
import subprocess
import time

from mercurial import (
    bundle2,
    changegroup,
    commands,
    discovery,
    dispatch,
    encoding,
    error,
    extensions,
    hg,
    localrepo,
    lock as lockmod,
    node,
    phases,
    policy,
    registrar,
    scmutil,
    templater,
    util,
)

# Mercurial
from mercurial.i18n import _

from . import bundleparts
from .. import shareutil


osutil = policy.importmod(r"osutil")

cmdtable = {}
command = registrar.command(cmdtable)

revsetpredicate = registrar.revsetpredicate()
templatekeyword = registrar.templatekeyword()
templatefunc = registrar.templatefunc()
localoverridesfile = "generated.infinitepushbackups.rc"
secondsinhour = 60 * 60

backupbookmarktuple = collections.namedtuple(
    "backupbookmarktuple", ["hostname", "reporoot", "localbookmark"]
)


class backupstate(object):
    def __init__(self):
        self.heads = util.sortdict()
        self.localbookmarks = util.sortdict()

    def empty(self):
        return not self.heads and not self.localbookmarks


class WrongPermissionsException(Exception):
    def __init__(self, logdir):
        self.logdir = logdir


restoreoptions = [
    ("", "reporoot", "", "root of the repo to restore"),
    ("", "user", "", "user who ran the backup"),
    ("", "hostname", "", "hostname of the repo to restore"),
]

_backuplockname = "infinitepushbackup.lock"

# Check if backup is enabled
def autobackupenabled(ui):
    # Backup is possibly disabled by user
    # but the disabling might have expired
    if ui.config("infinitepushbackup", "disableduntil", None) is not None:
        try:
            timestamp = int(ui.config("infinitepushbackup", "disableduntil"))
            if time.time() <= timestamp:
                return False
        except ValueError:
            # should never happen
            raise error.Abort(
                _(
                    "error: config file is broken, "
                    + "can't parse infinitepushbackup.disableduntil\n"
                )
            )
    # Backup may be unconditionally disabled by the Source Control Team
    return ui.configbool("infinitepushbackup", "autobackup")


# Wraps commands with backup if enabled
def extsetup(ui):
    if autobackupenabled(ui):
        extensions.wrapfunction(dispatch, "runcommand", _autobackupruncommandwrapper)
        extensions.wrapfunction(localrepo.localrepository, "transaction", _transaction)


def converttimestamptolocaltime(timestamp):
    _timeformat = "%Y-%m-%d %H:%M:%S %Z"
    return time.strftime(_timeformat, time.localtime(timestamp))


def checkinsertgeneratedconfig(localconfig, generatedconfig):
    includeline = "%include {generatedconfig}".format(generatedconfig=generatedconfig)

    # This split doesn't include '\n'
    if includeline in open(localconfig).read().splitlines():
        pass
    else:
        with open(localconfig, "a") as configfile:
            configfile.write("\n# include local overrides\n")
            configfile.write(includeline)
            configfile.write("\n")


@command("backupenable")
def backupenable(ui, repo, **opts):
    """
    Enable background backup

    Enables backups that have been disabled by `hg backupdisable`.
    """

    if autobackupenabled(ui):
        ui.write(_("background backup is already enabled\n"))
        return 0

    localconfig = repo.vfs.join("hgrc")
    generatedconfig = repo.vfs.join(localoverridesfile)
    checkinsertgeneratedconfig(localconfig, generatedconfig)
    with open(generatedconfig, "w") as file:
        file.write("")

    ui.write(_("background backup is enabled\n"))
    return 0


@command(
    "backupdisable", [("", "hours", "1", "disable backup for the specified duration")]
)
def backupdisable(ui, repo, **opts):
    """
    Disable background backup

    Sets the infinitepushbackup.disableduntil config option,
    which disables background backups for the specified duration.
    """

    if not autobackupenabled(ui):
        ui.write(_("note: background backup was already disabled\n"))

    localconfig = repo.vfs.join("hgrc")
    generatedconfig = repo.vfs.join(localoverridesfile)
    checkinsertgeneratedconfig(localconfig, generatedconfig)

    try:
        duration = secondsinhour * int(opts.get("hours", 1))
    except ValueError:
        raise error.Abort(
            _(
                "error: argument 'hours': invalid int value: '{value}'\n".format(
                    value=opts.get("hours")
                )
            )
        )

    timestamp = int(time.time()) + duration

    config = ConfigParser.ConfigParser()
    config.add_section("infinitepushbackup")
    config.set("infinitepushbackup", "disableduntil", timestamp)

    with open(generatedconfig, "w") as configfile:
        configfile.write("# disable infinitepush background backup\n")
        config.write(configfile)

    ui.write(
        _(
            "background backup is now disabled until {localtime}\n".format(
                localtime=converttimestamptolocaltime(timestamp)
            )
        )
    )
    return 0


@command("pushbackup", [("", "background", None, "run backup in background")])
def backup(ui, repo, dest=None, **opts):
    """
    Pushes commits, bookmarks and heads to infinitepush.
    New non-extinct commits are saved since the last `hg pushbackup`
    or since 0 revision if this backup is the first.
    Local bookmarks are saved remotely as:
        infinitepush/backups/USERNAME/HOST/REPOROOT/bookmarks/LOCAL_BOOKMARK
    Local heads are saved remotely as:
        infinitepush/backups/USERNAME/HOST/REPOROOT/heads/HEAD_HASH
    """
    if opts.get("background"):
        _dobackgroundbackup(ui, repo, dest)
        return 0

    try:
        # Wait at most 30 seconds, because that's the average backup time
        timeout = 30
        srcrepo = shareutil.getsrcrepo(repo)
        with lockmod.lock(srcrepo.vfs, _backuplockname, timeout=timeout):
            return _dobackup(ui, repo, dest, **opts)
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            ui.warn(_("timeout waiting on backup lock\n"))
            return 2
        else:
            raise


@command("pullbackup", restoreoptions)
def restore(ui, repo, dest=None, **opts):
    """
    Pulls commits from infinitepush that were previously saved with
    `hg pushbackup`.
    If user has only one backup for the `dest` repo then it will be restored.
    But user may have backed up many local repos that points to `dest` repo.
    These local repos may reside on different hosts or in different
    repo roots. It makes restore ambiguous; `--reporoot` and `--hostname`
    options are used to disambiguate.
    """

    other = _getremote(repo, ui, dest, **opts)

    sourcereporoot = opts.get("reporoot")
    sourcehostname = opts.get("hostname")
    namingmgr = BackupBookmarkNamingManager(ui, repo, opts.get("user"))
    allbackupstates = _downloadbackupstate(
        ui, other, sourcereporoot, sourcehostname, namingmgr
    )
    if not allbackupstates:
        ui.warn(_("no backups found!"))
        return 1
    _checkbackupstates(ui, namingmgr.username, allbackupstates)

    __, backupstate = allbackupstates.popitem()
    pullcmd, pullopts = _getcommandandoptions("^pull")
    # Pull backuped heads and nodes that are pointed by bookmarks.
    # Note that we are avoiding the use of set() because we want to pull
    # revisions in the same order
    pullopts["rev"] = list(backupstate.heads) + [
        x for x in backupstate.localbookmarks.values() if x not in backupstate.heads
    ]
    if dest:
        pullopts["source"] = dest

    maxrevbeforepull = len(repo.changelog)
    result = pullcmd(ui, repo, **pullopts)
    maxrevafterpull = len(repo.changelog)

    if ui.config("infinitepushbackup", "createlandedasmarkers", False):
        ext = extensions.find("pullcreatemarkers")
        ext.createmarkers(
            result, repo, maxrevbeforepull, maxrevafterpull, fromdrafts=False
        )

    with repo.wlock(), repo.lock(), repo.transaction("bookmark") as tr:
        changes = []
        for book, hexnode in backupstate.localbookmarks.iteritems():
            if hexnode in repo:
                changes.append((book, node.bin(hexnode)))
            else:
                ui.warn(_("%s not found, not creating %s bookmark") % (hexnode, book))
        repo._bookmarks.applychanges(repo, tr, changes)

    # manually write local backup state and flag to not autobackup
    # just after we restored, which would be pointless
    _writelocalbackupstate(
        repo.vfs, backupstate.heads.values(), backupstate.localbookmarks
    )
    repo.ignoreautobackup = True

    return result


@command(
    "getavailablebackups",
    [
        ("a", "all", None, _("list all backups, not just the most recent")),
        ("", "user", "", _("username, defaults to current user")),
        ("", "json", None, _("print available backups in json format")),
    ],
)
def getavailablebackups(ui, repo, dest=None, **opts):
    other = _getremote(repo, ui, dest, **opts)

    sourcereporoot = opts.get("reporoot")
    sourcehostname = opts.get("hostname")

    namingmgr = BackupBookmarkNamingManager(ui, repo, opts.get("user"))
    allbackupstates = _downloadbackupstate(
        ui, other, sourcereporoot, sourcehostname, namingmgr
    )

    # Preserve allbackupstates MRU order in messages for users
    if opts.get("json"):
        jsondict = util.sortdict()
        for hostname, reporoot in allbackupstates.keys():
            jsondict.setdefault(hostname, []).append(reporoot)
        ui.write("%s\n" % json.dumps(jsondict, indent=4))
    elif not allbackupstates:
        ui.write(_("no backups available for %s\n") % namingmgr.username)
    else:
        _printbackupstates(
            ui, namingmgr.username, allbackupstates, bool(opts.get("all"))
        )


@command(
    "backupdelete",
    [
        ("", "reporoot", "", "root of the repo to delete the backup for"),
        ("", "hostname", "", "hostname of the repo to delete the backup for"),
    ],
)
def backupdelete(ui, repo, dest=None, **opts):
    """
    Deletes a backup from the server.  Removes all heads and bookmarks
    associated with the backup from the server.  The commits themselves are
    not removed, so you can still update to them using 'hg update HASH'.
    """
    sourcereporoot = opts.get("reporoot")
    sourcehostname = opts.get("hostname")
    if not sourcereporoot or not sourcehostname:
        msg = _("you must specify a reporoot and hostname to delete a backup")
        hint = _("use 'hg getavailablebackups' to find which backups exist")
        raise error.Abort(msg, hint=hint)
    namingmgr = BackupBookmarkNamingManager(ui, repo)

    # Do some sanity checking on the names
    if not re.match(r"^[-a-zA-Z0-9._/]+$", sourcereporoot):
        msg = _("repo root contains unexpected characters")
        raise error.Abort(msg)
    if not re.match(r"^[-a-zA-Z0-9.]+$", sourcehostname):
        msg = _("hostname contains unexpected characters")
        raise error.Abort(msg)
    if sourcereporoot == repo.origroot and sourcehostname == namingmgr.hostname:
        ui.warn(_("warning: this backup matches the current repo\n"))

    other = _getremote(repo, ui, dest, **opts)
    backupstates = _downloadbackupstate(
        ui, other, sourcereporoot, sourcehostname, namingmgr
    )
    backupstate = backupstates.get((sourcehostname, sourcereporoot))
    if backupstate is None:
        raise error.Abort(
            _("no backup found for %s on %s") % (sourcereporoot, sourcehostname)
        )
    ui.write(_("%s on %s:\n") % (sourcereporoot, sourcehostname))
    ui.write(_("    heads:\n"))
    for head in backupstate.heads:
        ui.write(("        %s\n") % head)
    ui.write(_("    bookmarks:\n"))
    for bookname, booknode in backupstate.localbookmarks.items():
        ui.write(("        %-20s %s\n") % (bookname + ":", booknode))
    if ui.promptchoice(_("delete this backup (yn)? $$ &Yes $$ &No"), 1) == 0:
        ui.status(
            _("deleting backup for %s on %s\n") % (sourcereporoot, sourcehostname)
        )
        bookmarks = {
            namingmgr.getcommonuserhostreporootprefix(
                sourcehostname, sourcereporoot
            ): ""
        }
        _dobackuppush(ui, repo, other, None, bookmarks)
        ui.status(_("backup deleted\n"))
        ui.status(
            _("(you can still access the commits directly " "using their hashes)\n")
        )
    return 0


@command(
    "debugcheckbackup",
    [("", "all", None, _("check all backups that user have"))] + restoreoptions,
)
def checkbackup(ui, repo, dest=None, **opts):
    """
    Checks that all the nodes that backup needs are available in bundlestore
    This command can check either specific backup (see restoreoptions) or all
    backups for the user
    """

    sourcereporoot = opts.get("reporoot")
    sourcehostname = opts.get("hostname")

    other = _getremote(repo, ui, dest, **opts)
    namingmgr = BackupBookmarkNamingManager(ui, repo, opts.get("user"))
    allbackupstates = _downloadbackupstate(
        ui, other, sourcereporoot, sourcehostname, namingmgr
    )
    if not opts.get("all"):
        _checkbackupstates(ui, namingmgr.username, allbackupstates)

    ret = 0
    while allbackupstates:
        key, bkpstate = allbackupstates.popitem()
        ui.status(_("checking %s on %s\n") % (key[1], key[0]))
        if not _dobackupcheck(bkpstate, ui, repo, dest, **opts):
            ret = 255
    return ret


@command("debugwaitbackup", [("", "timeout", "", "timeout value")])
def waitbackup(ui, repo, timeout):
    try:
        if timeout:
            timeout = int(timeout)
        else:
            timeout = -1
    except ValueError:
        raise error.Abort("timeout should be integer")

    try:
        repo = shareutil.getsrcrepo(repo)
        with lockmod.lock(repo.vfs, _backuplockname, timeout=timeout):
            pass
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            raise error.Abort(_("timeout while waiting for backup"))
        raise


@command(
    "isbackedup",
    [
        ("r", "rev", [], _("show the specified revision or revset"), _("REV")),
        ("", "remote", None, _("check on the remote server")),
    ],
)
def isbackedup(ui, repo, dest=None, **opts):
    """checks if commit was backed up to infinitepush

    If no revision are specified then it checks working copy parent
    """

    revs = opts.get("rev")
    remote = opts.get("remote")
    if not revs:
        revs = ["."]
    bkpstate = _readlocalbackupstate(ui, repo)
    unfi = repo.unfiltered()
    backeduprevs = unfi.revs("draft() and ::%ls", bkpstate.heads)
    if remote:
        other = _getremote(repo, ui, dest, **opts)
    for r in scmutil.revrange(unfi, revs):
        ui.write(_(unfi[r].hex() + " "))
        backedup = r in backeduprevs
        if remote and backedup:
            try:
                other.lookup(unfi[r].hex())
            except error.RepoError:
                backedup = False
        ui.write(_("backed up" if backedup else "not backed up"))
        ui.write(_("\n"))


@revsetpredicate("backedup")
def backedup(repo, subset, x):
    """Draft changesets that have been backed up by infinitepush"""
    unfi = repo.unfiltered()
    bkpstate = _readlocalbackupstate(repo.ui, repo)
    return subset & unfi.revs("draft() and ::%ls and not hidden()", bkpstate.heads)


@revsetpredicate("notbackedup")
def notbackedup(repo, subset, x):
    """Changesets that have not yet been backed up by infinitepush"""
    bkpstate = _readlocalbackupstate(repo.ui, repo)
    bkpheads = set(bkpstate.heads)
    candidates = set(_backupheads(repo.ui, repo))
    notbackeduprevs = set()
    # Find all revisions that are ancestors of the expected backup heads,
    # stopping when we reach either a public commit or a known backup head.
    while candidates:
        candidate = candidates.pop()
        if candidate not in bkpheads:
            ctx = repo[candidate]
            rev = ctx.rev()
            if rev not in notbackeduprevs and ctx.phase() != phases.public:
                # This rev may not have been backed up.  Record it, and add its
                # parents as candidates.
                notbackeduprevs.add(rev)
                candidates.update([p.hex() for p in ctx.parents()])
    if notbackeduprevs:
        # Some revisions in this set may actually have been backed up by
        # virtue of being an ancestor of a different backup head, which may
        # have been hidden since the backup was made.  Find these and remove
        # them from the set.
        unfi = repo.unfiltered()
        candidates = bkpheads
        while candidates:
            candidate = candidates.pop()
            if candidate in unfi:
                ctx = unfi[candidate]
                if ctx.phase() != phases.public:
                    notbackeduprevs.discard(ctx.rev())
                    candidates.update([p.hex() for p in ctx.parents()])
    return subset & notbackeduprevs


@templatekeyword("backingup")
def backingup(repo, ctx, **args):
    """Whether infinitepush is currently backing up commits."""
    # If the backup lock exists then a backup should be in progress.
    srcrepo = shareutil.getsrcrepo(repo)
    return srcrepo.vfs.lexists(_backuplockname)


def _smartlogbackupsuggestion(ui, repo):
    ui.warn(
        _(
            "Run `hg pushbackup` to perform a backup. "
            "If this fails,\n"
            "please report to the Source Control @ FB group.\n"
        )
    )


def _smartlogbackupmessagemap(ui, repo):
    return {
        "inprogress": "backing up",
        "pending": "backup pending",
        "failed": "not backed up",
    }


@templatefunc("backupstatusmsg(status)")
def backupstatusmsg(context, mapping, args):
    if len(args) != 1:
        raise error.ParseError(_("backupstatusmsg expects 1 argument"))
    status = templater.evalfuncarg(context, mapping, args[0])
    repo = mapping["ctx"].repo()
    wordmap = _smartlogbackupmessagemap(repo.ui, repo)
    if status not in wordmap:
        raise error.ParseError(_("unknown status"))
    return wordmap[status]


def smartlogsummary(ui, repo):
    if not ui.configbool("infinitepushbackup", "enablestatus"):
        return

    # Output backup status if enablestatus is on
    autobackupenabledstatus = autobackupenabled(ui)
    if not autobackupenabledstatus:
        timestamp = ui.config("infinitepushbackup", "disableduntil", None)
        if timestamp:
            ui.write(
                _(
                    "note: background backup is currently disabled until "
                    + converttimestamptolocaltime(int(timestamp))
                    + "\n"
                )
            )
            ui.write(_("so your commits are not being backed up.\n"))
            ui.write(_("Run `hg backupenable` to turn backups back on.\n"))
        else:
            ui.write(
                _(
                    "note: background backup is currently disabled "
                    + "by the Source Control Team,\n"
                )
            )
            ui.write(_("so your commits are not being backed up.\n"))

    # Don't output the summary if a backup is currently in progress.
    srcrepo = shareutil.getsrcrepo(repo)
    if srcrepo.vfs.lexists(_backuplockname):
        return

    unbackeduprevs = repo.revs("notbackedup()")

    # Count the number of changesets that haven't been backed up for 10 minutes.
    # If there is only one, also print out its hash.
    backuptime = time.time() - 10 * 60  # 10 minutes ago
    count = 0
    singleunbackeduprev = None
    for rev in unbackeduprevs:
        if repo[rev].date()[0] <= backuptime:
            singleunbackeduprev = rev
            count += 1
    if count > 0:
        if not autobackupenabledstatus:
            ui.write("\n")
        if count > 1:
            ui.warn(_("note: %d changesets are not backed up.\n") % count)
        else:
            ui.warn(
                _("note: changeset %s is not backed up.\n")
                % node.short(repo[singleunbackeduprev].node())
            )
        _smartlogbackupsuggestion(ui, repo)


def _autobackupruncommandwrapper(orig, lui, repo, cmd, fullargs, *args):
    """
    If this wrapper is enabled then auto backup is started after every command
    that modifies a repository.
    Since we don't want to start auto backup after read-only commands,
    then this wrapper checks if this command opened at least one transaction.
    If yes then background backup will be started.
    """

    # For chg, do not wrap the "serve" runcommand call
    if "CHGINTERNALMARK" in encoding.environ:
        return orig(lui, repo, cmd, fullargs, *args)

    try:
        return orig(lui, repo, cmd, fullargs, *args)
    finally:
        if getattr(repo, "txnwasopened", False) and not getattr(
            repo, "ignoreautobackup", False
        ):
            lui.debug("starting infinitepush autobackup in the background\n")
            _dobackgroundbackup(lui, repo)


def _transaction(orig, self, *args, **kwargs):
    """ Wrapper that records if a transaction was opened.

    If a transaction was opened then we want to start background backup process.
    This hook records the fact that transaction was opened.
    """
    self.txnwasopened = True
    return orig(self, *args, **kwargs)


def _backupheads(ui, repo):
    """Returns the set of heads that should be backed up in this repo."""
    maxheadstobackup = ui.configint("infinitepushbackup", "maxheadstobackup", -1)

    revset = "heads(draft()) & not obsolete()"

    backupheads = [ctx.hex() for ctx in repo.set(revset)]
    if maxheadstobackup > 0:
        backupheads = backupheads[-maxheadstobackup:]
    elif maxheadstobackup == 0:
        backupheads = []
    return set(backupheads)


def _dobackup(ui, repo, dest, **opts):
    ui.status(_("starting backup %s\n") % time.strftime("%H:%M:%S %d %b %Y %Z"))
    start = time.time()
    # to handle multiple working copies correctly
    repo = shareutil.getsrcrepo(repo)
    currentbkpgenerationvalue = _readbackupgenerationfile(repo.vfs)
    newbkpgenerationvalue = ui.configint("infinitepushbackup", "backupgeneration", 0)
    if currentbkpgenerationvalue != newbkpgenerationvalue:
        # Unlinking local backup state will trigger re-backuping
        _deletebackupstate(repo)
        _writebackupgenerationfile(repo.vfs, newbkpgenerationvalue)
    bkpstate = _readlocalbackupstate(ui, repo)

    # this variable stores the local store info (tip numeric revision and date)
    # which we use to quickly tell if our backup is stale
    afterbackupinfo = _getlocalinfo(repo)

    # This variable will store what heads will be saved in backup state file
    # if backup finishes successfully
    afterbackupheads = _backupheads(ui, repo)
    other = _getremote(repo, ui, dest, **opts)
    outgoing, badhexnodes = _getrevstobackup(
        repo, ui, other, afterbackupheads - set(bkpstate.heads)
    )
    # If remotefilelog extension is enabled then there can be nodes that we
    # can't backup. In this case let's remove them from afterbackupheads
    afterbackupheads.difference_update(badhexnodes)

    # Similar to afterbackupheads, this variable stores what bookmarks will be
    # saved in backup state file if backup finishes successfully
    afterbackuplocalbooks = _getlocalbookmarks(repo)
    afterbackuplocalbooks = _filterbookmarks(
        afterbackuplocalbooks, repo, afterbackupheads
    )

    newheads = afterbackupheads - set(bkpstate.heads)
    removedheads = set(bkpstate.heads) - afterbackupheads
    newbookmarks = _dictdiff(afterbackuplocalbooks, bkpstate.localbookmarks)
    removedbookmarks = _dictdiff(bkpstate.localbookmarks, afterbackuplocalbooks)

    namingmgr = BackupBookmarkNamingManager(ui, repo)
    bookmarkstobackup = _getbookmarkstobackup(
        repo, newbookmarks, removedbookmarks, newheads, removedheads, namingmgr
    )

    # Special cases if backup state is empty.
    if bkpstate.empty():
        # If there is nothing to backup, exit now to prevent accidentally
        # clearing a previous backup.
        if not afterbackuplocalbooks and not afterbackupheads:
            ui.status(_("nothing to backup\n"))
            return
        # Otherwise, clean all backup bookmarks from the server.
        bookmarkstobackup[namingmgr.getbackupheadprefix()] = ""
        bookmarkstobackup[namingmgr.getbackupbookmarkprefix()] = ""

    try:
        if _dobackuppush(ui, repo, other, outgoing, bookmarkstobackup):
            _writelocalbackupstate(
                repo.vfs, list(afterbackupheads), afterbackuplocalbooks
            )
            if ui.config("infinitepushbackup", "savelatestbackupinfo"):
                _writelocalbackupinfo(repo.vfs, **afterbackupinfo)
        else:
            ui.status(_("nothing to backup\n"))
    finally:
        ui.status(_("finished in %f seconds\n") % (time.time() - start))


def _dobackuppush(ui, repo, other, outgoing, bookmarks):
    # Wrap deltaparent function to make sure that bundle takes less space
    # See _deltaparent comments for details
    extensions.wrapfunction(changegroup.cg2packer, "deltaparent", _deltaparent)
    try:
        bundler = _createbundler(ui, repo, other)
        bundler.addparam("infinitepush", "True")
        backup = False
        if outgoing and outgoing.missing:
            backup = True
            parts = bundleparts.getscratchbranchparts(
                repo,
                other,
                outgoing,
                confignonforwardmove=False,
                ui=ui,
                bookmark=None,
                create=False,
            )
            for part in parts:
                bundler.addpart(part)

        if bookmarks:
            backup = True
            bundler.addpart(bundleparts.getscratchbookmarkspart(other, bookmarks))

        if backup:
            _sendbundle(bundler, other)
        return backup
    finally:
        # cleanup ensures that all pipes are flushed
        cleanup = getattr(other, "_cleanup", None) or getattr(other, "cleanup")
        try:
            cleanup()
        except Exception:
            ui.warn(_("remote connection cleanup failed\n"))
        extensions.unwrapfunction(changegroup.cg2packer, "deltaparent", _deltaparent)
    return 0


def _dobackgroundbackup(ui, repo, dest=None, command=None):
    background_cmd = command or ["hg", "pushbackup"]
    infinitepush_bgssh = ui.config("infinitepush", "bgssh")
    if infinitepush_bgssh:
        background_cmd += ["--config", "ui.ssh=%s" % infinitepush_bgssh]

    if ui.configbool("infinitepushbackup", "bgdebug", False):
        background_cmd.append("--debug")

    if dest:
        background_cmd.append(dest)
    logfile = None
    logdir = ui.config("infinitepushbackup", "logdir")
    if logdir:
        # make newly created files and dirs non-writable
        oldumask = os.umask(0o022)
        try:
            try:
                username = util.shortuser(ui.username())
            except Exception:
                username = "unknown"

            if not _checkcommonlogdir(logdir):
                raise WrongPermissionsException(logdir)

            userlogdir = os.path.join(logdir, username)
            util.makedirs(userlogdir)

            if not _checkuserlogdir(userlogdir):
                raise WrongPermissionsException(userlogdir)

            reporoot = repo.origroot
            reponame = os.path.basename(reporoot)
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

    with open(logfile, "a") as f:
        subprocess.Popen(
            background_cmd, shell=False, stdout=f, stderr=subprocess.STDOUT
        )


def _dobackupcheck(bkpstate, ui, repo, dest, **opts):
    remotehexnodes = sorted(set(bkpstate.heads).union(bkpstate.localbookmarks.values()))
    if not remotehexnodes:
        return True
    other = _getremote(repo, ui, dest, **opts)
    batch = other.iterbatch()
    for hexnode in remotehexnodes:
        batch.lookup(hexnode)
    batch.submit()
    lookupresults = batch.results()
    i = 0
    try:
        for i, r in enumerate(lookupresults):
            # iterate over results to make it throw if revision
            # was not found
            pass
        return True
    except error.RepoError:
        ui.warn(_("unknown revision %r\n") % remotehexnodes[i])
        return False


_backuplatestinfofile = "infinitepushlatestbackupinfo"
_backupstatefile = "infinitepushbackupstate"
_backupgenerationfile = "infinitepushbackupgeneration"

# Common helper functions
def _getlocalinfo(repo):
    localinfo = {}
    localinfo["rev"] = repo[repo.changelog.tip()].rev()
    localinfo["time"] = int(time.time())
    return localinfo


def _getlocalbookmarks(repo):
    localbookmarks = {}
    for bookmark, data in repo._bookmarks.iteritems():
        hexnode = node.hex(data)
        localbookmarks[bookmark] = hexnode
    return localbookmarks


def _filterbookmarks(localbookmarks, repo, headstobackup):
    """Filters out some bookmarks from being backed up

    Filters out bookmarks that do not point to ancestors of headstobackup or
    public commits
    """

    headrevstobackup = [repo[hexhead].rev() for hexhead in headstobackup]
    ancestors = repo.changelog.ancestors(headrevstobackup, inclusive=True)
    filteredbooks = {}
    for bookmark, hexnode in localbookmarks.iteritems():
        if repo[hexnode].rev() in ancestors or repo[hexnode].phase() == phases.public:
            filteredbooks[bookmark] = hexnode
    return filteredbooks


def _downloadbackupstate(ui, other, sourcereporoot, sourcehostname, namingmgr):
    """
    Sqlindex returns backups in order of insertion

    _downloadbackupstate returns an ordered dict
        <host, reporoot> => backups
                            that contains
                                * heads (ordered dict)
                                * localbookmarks (ordered dict)

    Hostnames and reporoot in the dict should be in MRU order (most recent used)
    So, the fresher backups come first
    Internally backups preserve the order of insertion

    Note:

    Fileindex returns backups in lexicographical order since fileindexapi
    don't support yet returning bookmarks in the order of insertion
    Hostnames and reporoot will not be nicely MRU ordered
    until the order of insertion is not supported in fileindex
    """
    if sourcehostname and sourcereporoot:
        pattern = namingmgr.getcommonuserhostreporootprefix(
            sourcehostname, sourcereporoot
        )
    elif sourcehostname:
        pattern = namingmgr.getcommonuserhostprefix(sourcehostname)
    else:
        pattern = namingmgr.getcommonuserprefix()

    fetchedbookmarks = other.listkeyspatterns("bookmarks", patterns=[pattern])
    allbackupstates = util.sortdict()
    for book, hexnode in fetchedbookmarks.iteritems():
        parsed = _parsebackupbookmark(book, namingmgr)
        if parsed:
            if sourcereporoot and sourcereporoot != parsed.reporoot:
                continue
            if sourcehostname and sourcehostname != parsed.hostname:
                continue
            key = (parsed.hostname, parsed.reporoot)
            if key not in allbackupstates:
                allbackupstates[key] = backupstate()
            if parsed.localbookmark:
                bookname = parsed.localbookmark
                allbackupstates[key].localbookmarks[bookname] = hexnode
            else:
                allbackupstates[key].heads[hexnode] = hexnode
        else:
            ui.warn(_("wrong format of backup bookmark: %s") % book)

    # reverse to make MRU order
    allbackupstatesrev = util.sortdict()
    for key, value in reversed(allbackupstates.items()):
        allbackupstatesrev[key] = value

    return allbackupstatesrev


def _checkbackupstates(ui, username, allbackupstates):
    if not allbackupstates:
        raise error.Abort("no backups found!")

    if len(allbackupstates) > 1:
        _printbackupstates(ui, username, allbackupstates)
        raise error.Abort(
            _("multiple backups found"),
            hint=_("set --hostname and --reporoot to pick a backup"),
        )


def _printbackupstates(ui, username, allbackupstates, all=False):
    ui.write(
        _(
            "user %s has %d available backups:\n"
            "(backups are ordered with "
            "the most recent at the top of the list)\n"
        )
        % (username, len(allbackupstates))
    )

    limit = ui.configint("infinitepushbackup", "backuplistlimit", 5)
    for i, (hostname, reporoot) in enumerate(allbackupstates.keys()):
        if not all and i == limit:
            ui.write(
                _(
                    "(older backups have been hidden, "
                    "run 'hg getavailablebackups --all' to see them all)\n"
                )
            )
            break
        ui.write(_("%s on %s\n") % (reporoot, hostname))


class BackupBookmarkNamingManager(object):
    """
    The naming convention is:
    infinitepush/backups/<unixusername>/<host>/<reporoot>/bookmarks/<name>
    or:
    infinitepush/backups/<unixusername>/<host>/<reporoot>/heads/<hash>
    """

    def __init__(self, ui, repo, username=None):
        self.ui = ui
        self.repo = repo
        if not username:
            username = util.shortuser(ui.username())
        self.username = username

        self.hostname = self.ui.config("infinitepushbackup", "hostname")
        if not self.hostname:
            self.hostname = socket.gethostname()

    def getcommonuserprefix(self):
        return "/".join((self._getcommonuserprefix(), "*"))

    def getcommonuserhostprefix(self, host):
        return "/".join((self._getcommonuserprefix(), host, "*"))

    def getcommonuserhostreporootprefix(self, host, reporoot):
        # Remove any prefix or suffix slashes, since the join will add them
        # back and the format doesn't expect a double slash.
        strippedroot = reporoot.strip("/")
        return "/".join((self._getcommonuserprefix(), host, strippedroot, "*"))

    def getcommonprefix(self):
        return "/".join((self._getcommonprefix(), "*"))

    def getbackupbookmarkprefix(self):
        return "/".join((self._getbackupbookmarkprefix(), "*"))

    def getbackupbookmarkname(self, bookmark):
        bookmark = _escapebookmark(bookmark)
        return "/".join((self._getbackupbookmarkprefix(), bookmark))

    def getbackupheadprefix(self):
        return "/".join((self._getbackupheadprefix(), "*"))

    def getbackupheadname(self, hexhead):
        return "/".join((self._getbackupheadprefix(), hexhead))

    def _getbackupbookmarkprefix(self):
        return "/".join((self._getcommonprefix(), "bookmarks"))

    def _getbackupheadprefix(self):
        return "/".join((self._getcommonprefix(), "heads"))

    def _getcommonuserprefix(self):
        return "/".join(("infinitepush", "backups", self.username))

    def _getcommonprefix(self):
        reporoot = self.repo.origroot

        result = "/".join((self._getcommonuserprefix(), self.hostname))
        if not reporoot.startswith("/"):
            result += "/"
        result += reporoot
        if result.endswith("/"):
            result = result[:-1]
        return result


def _escapebookmark(bookmark):
    """
    If `bookmark` contains "bookmarks" as a substring then replace it with
    "bookmarksbookmarks". This will make parsing remote bookmark name
    unambigious.
    Also, encode * since it is used for prefix pattern matching
    """
    bookmark = encoding.fromlocal(bookmark)
    bookmark = bookmark.replace("*", "*%")
    return bookmark.replace("bookmarks", "bookmarksbookmarks")


def _unescapebookmark(bookmark):
    bookmark = encoding.tolocal(bookmark)
    bookmark = bookmark.replace("*%", "*")
    return bookmark.replace("bookmarksbookmarks", "bookmarks")


def _getremote(repo, ui, dest, **opts):
    path = ui.paths.getpath(dest, default=("infinitepush", "default"))
    if not path:
        raise error.Abort(
            _("default repository not configured!"),
            hint=_("see 'hg help config.paths'"),
        )
    dest = path.pushloc or path.loc
    return hg.peer(repo, opts, dest)


def _getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts


# Backup helper functions


def _deltaparent(orig, self, revlog, rev, p1, p2, prev):
    # This version of deltaparent prefers p1 over prev to use less space
    dp = revlog.deltaparent(rev)
    if dp == node.nullrev and not revlog.storedeltachains:
        # send full snapshot only if revlog configured to do so
        return node.nullrev
    return p1


def _getbookmarkstobackup(
    repo, newbookmarks, removedbookmarks, newheads, removedheads, namingmgr
):
    bookmarkstobackup = {}

    for bookmark, hexnode in removedbookmarks.items():
        backupbookmark = namingmgr.getbackupbookmarkname(bookmark)
        bookmarkstobackup[backupbookmark] = ""

    for bookmark, hexnode in newbookmarks.items():
        backupbookmark = namingmgr.getbackupbookmarkname(bookmark)
        bookmarkstobackup[backupbookmark] = hexnode

    for hexhead in removedheads:
        headbookmarksname = namingmgr.getbackupheadname(hexhead)
        bookmarkstobackup[headbookmarksname] = ""

    for hexhead in newheads:
        headbookmarksname = namingmgr.getbackupheadname(hexhead)
        bookmarkstobackup[headbookmarksname] = hexhead

    return bookmarkstobackup


def _createbundler(ui, repo, other):
    bundler = bundle2.bundle20(ui, bundle2.bundle2caps(other))
    compress = ui.config("infinitepush", "bundlecompression", "UN")
    bundler.setcompression(compress)
    # Disallow pushback because we want to avoid taking repo locks.
    # And we don't need pushback anyway
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo, allowpushback=False))
    bundler.newpart("replycaps", data=capsblob)
    return bundler


def _sendbundle(bundler, other):
    stream = util.chunkbuffer(bundler.getchunks())
    try:
        reply = other.unbundle(stream, ["force"], other.url())
        # Look for an error part in the response.  Note that we don't apply
        # the reply bundle, as we're not expecting any response, except maybe
        # an error.  If we receive any extra parts, that is an error.
        for part in reply.iterparts():
            if part.type == "error:abort":
                raise bundle2.AbortFromPart(
                    part.params["message"], hint=part.params.get("hint")
                )
            elif part.type == "reply:changegroup":
                pass
            else:
                raise error.Abort(_("unexpected part in reply: %s") % part.type)
    except error.BundleValueError as exc:
        raise error.Abort(_("missing support for %s") % exc)


def findcommonoutgoing(repo, ui, other, heads):
    if heads:
        # Avoid using remotenames fastheaddiscovery heuristic. It uses
        # remotenames file to quickly find commonoutgoing set, but it can
        # result in sending public commits to infinitepush servers.
        # For example:
        #
        #        o draft
        #       /
        #      o C1
        #      |
        #     ...
        #      |
        #      o remote/master
        #
        # pushbackup in that case results in sending to the infinitepush server
        # all public commits from 'remote/master' to C1. It increases size of
        # the bundle + it may result in storing data about public commits
        # in infinitepush table.

        with ui.configoverride({("remotenames", "fastheaddiscovery"): False}):
            nodes = map(repo.changelog.node, heads)
            return discovery.findcommonoutgoing(repo, other, onlyheads=nodes)
    else:
        return None


def _getrevstobackup(repo, ui, other, headstobackup):
    # In rare cases it's possible to have a local node without filelogs.
    # This is possible if remotefilelog is enabled and if the node was
    # stripped server-side. We want to filter out these bad nodes and all
    # of their descendants.
    badnodes = ui.configlist("infinitepushbackup", "dontbackupnodes", [])
    badnodes = [node for node in badnodes if node in repo]
    badrevs = [repo[node].rev() for node in badnodes]
    badnodesdescendants = repo.set("%ld::", badrevs) if badrevs else set()
    badnodesdescendants = set(ctx.hex() for ctx in badnodesdescendants)
    filteredheads = filter(lambda head: head in badnodesdescendants, headstobackup)

    if filteredheads:
        ui.warn(_("filtering nodes: %s\n") % filteredheads)
        ui.log(
            "infinitepushbackup",
            "corrupted nodes found",
            infinitepushbackupcorruptednodes="failure",
        )
    headstobackup = filter(lambda head: head not in badnodesdescendants, headstobackup)

    revs = list(repo[hexnode].rev() for hexnode in headstobackup)
    outgoing = findcommonoutgoing(repo, ui, other, revs)
    nodeslimit = 1000
    if outgoing and len(outgoing.missing) > nodeslimit:
        # trying to push too many nodes usually means that there is a bug
        # somewhere. Let's be safe and avoid pushing too many nodes at once
        raise error.Abort(
            "trying to back up too many nodes: %d" % (len(outgoing.missing),)
        )
    return outgoing, set(filteredheads)


def _localbackupstateexists(repo):
    return repo.vfs.exists(_backupstatefile)


def _deletebackupstate(repo):
    return repo.vfs.tryunlink(_backupstatefile)


def _readlocalbackupstate(ui, repo):
    repo = shareutil.getsrcrepo(repo)
    if not _localbackupstateexists(repo):
        return backupstate()

    with repo.vfs(_backupstatefile) as f:
        try:
            state = json.loads(f.read())
            if not isinstance(state["bookmarks"], dict) or not isinstance(
                state["heads"], list
            ):
                raise ValueError("bad types of bookmarks or heads")

            result = backupstate()
            result.heads = set(map(str, state["heads"]))
            result.localbookmarks = state["bookmarks"]
            return result
        except (ValueError, KeyError, TypeError) as e:
            ui.warn(_("corrupt file: %s (%s)\n") % (_backupstatefile, e))
            return backupstate()
    return backupstate()


def _writelocalbackupstate(vfs, heads, bookmarks):
    with vfs(_backupstatefile, "w") as f:
        f.write(json.dumps({"heads": list(heads), "bookmarks": bookmarks}))


def _readbackupgenerationfile(vfs):
    try:
        with vfs(_backupgenerationfile) as f:
            return int(f.read())
    except (IOError, OSError, ValueError):
        return 0


def _writebackupgenerationfile(vfs, backupgenerationvalue):
    with vfs(_backupgenerationfile, "w", atomictemp=True) as f:
        f.write(str(backupgenerationvalue))


def _writelocalbackupinfo(vfs, rev, time):
    with vfs(_backuplatestinfofile, "w", atomictemp=True) as f:
        f.write(("backuprevision=%d\nbackuptime=%d\n") % (rev, time))


# Restore helper functions
def _parsebackupbookmark(backupbookmark, namingmgr):
    """Parses backup bookmark and returns info about it

    Backup bookmark may represent either a local bookmark or a head.
    Returns None if backup bookmark has wrong format or tuple.
    First entry is a hostname where this bookmark came from.
    Second entry is a root of the repo where this bookmark came from.
    Third entry in a tuple is local bookmark if backup bookmark
    represents a local bookmark and None otherwise.
    """

    backupbookmarkprefix = namingmgr._getcommonuserprefix()
    commonre = "^{0}/([-\w.]+)(/.*)".format(re.escape(backupbookmarkprefix))
    bookmarkre = commonre + "/bookmarks/(.*)$"
    headsre = commonre + "/heads/[a-f0-9]{40}$"

    match = re.search(bookmarkre, backupbookmark)
    if not match:
        match = re.search(headsre, backupbookmark)
        if not match:
            return None
        # It's a local head not a local bookmark.
        # That's why localbookmark is None
        return backupbookmarktuple(
            hostname=match.group(1), reporoot=match.group(2), localbookmark=None
        )

    return backupbookmarktuple(
        hostname=match.group(1),
        reporoot=match.group(2),
        localbookmark=_unescapebookmark(match.group(3)),
    )


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
    for entry in osutil.listdir(userlogdir):
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


def _dictdiff(first, second):
    """Returns new dict that contains items from the first dict that are missing
    from the second dict.
    """
    result = {}
    for book, hexnode in first.items():
        if second.get(book) != hexnode:
            result[book] = hexnode
    return result
