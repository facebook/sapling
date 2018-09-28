# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" back up draft commits in the cloud

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
from mercurial.i18n import _


osutil = policy.importmod(r"osutil")
infinitepush = None

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
    try:
        infinitepushmod = extensions.find("infinitepush")
    except KeyError:
        msg = _("The infinitepushbackup extension requires the infinitepush extension")
        raise error.Abort(msg)

    global infinitepush
    infinitepush = infinitepushmod

    # Allow writing backup files outside the normal lock
    localrepo.localrepository._wlockfreeprefix.update(
        [_backupstatefile, _backupgenerationfile]
    )

    if autobackupenabled(ui):
        extensions.wrapfunction(dispatch, "runcommand", _autobackupruncommandwrapper)
        extensions.wrapfunction(localrepo.localrepository, "transaction", _transaction)

    def wrapsmartlog(loaded):
        if not loaded:
            return
        smartlogmod = extensions.find("smartlog")
        extensions.wrapcommand(smartlogmod.cmdtable, "smartlog", _smartlog)

    extensions.afterloaded("smartlog", wrapsmartlog)


def _smartlog(orig, ui, repo, **opts):
    res = orig(ui, repo, **opts)
    smartlogsummary(ui, repo)
    return res


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

    localconfig = repo.localvfs.join("hgrc")
    generatedconfig = repo.localvfs.join(localoverridesfile)
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

    localconfig = repo.localvfs.join("hgrc")
    generatedconfig = repo.localvfs.join(localoverridesfile)
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
    try:
        with lockmod.trylock(ui, repo.sharedvfs, _backuplockname, 0, 0):
            pass
    except error.LockHeld as e:
        if e.lockinfo.isrunning():
            ui.warn(
                _(
                    "warning: disable does not affect the running backup process\n"
                    "kill the process (pid %s on %s) gracefully if needed\n"
                )
                % (e.lockinfo.uniqueid, e.lockinfo.namespace)
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
        with lockmod.lock(repo.sharedvfs, _backuplockname, timeout=timeout):
            _dobackup(ui, repo, dest, **opts)
            return 0
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
        repo.sharedvfs, backupstate.heads.values(), backupstate.localbookmarks
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
    if sourcereporoot == repo.sharedroot and sourcehostname == namingmgr.hostname:
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
        infinitepush.pushbackupbundle(ui, repo, other, None, bookmarks)
        ui.status(_("backup deleted\n"))
        ui.status(_("(you can still access the commits directly using their hashes)\n"))
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
        with lockmod.lock(repo.sharedvfs, _backuplockname, timeout=timeout):
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
def backingup(repo, **args):
    """Whether infinitepush is currently backing up commits."""
    # If the backup lock exists then a backup should be in progress.
    return _islocked(repo)


def _islocked(repo):
    path = repo.sharedvfs.join(_backuplockname)
    return util.islocked(path)


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


def _smartlogbackuphealthcheckmsg(ui, repo):
    return


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
    else:
        _smartlogbackuphealthcheckmsg(ui, repo)

    # Don't output the summary if a backup is currently in progress.
    if _islocked(repo):
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


def _filterbadnodes(ui, repo, heads):
    """Remove bad nodes from the set of draft ancestors of heads."""
    badnodes = ui.configlist("infinitepushbackup", "dontbackupnodes", [])
    badnodes = [n for n in badnodes if n in repo]

    # Work out which badnodes are also draft ancestors of the heads we're
    # interested in.
    badnodes = [
        ctx.hex() for ctx in repo.set("draft() & ::%ls & %ls::", heads, badnodes)
    ]

    if badnodes:
        # Log that we're filtering these commits.
        ui.warn(_("not backing up commits marked as bad: %s\n") % ", ".join(badnodes))
        ui.log(
            "infinitepushbackup",
            "corrupted nodes found",
            infinitepushbackupcorruptednodes="failure",
        )

        # Return a new set of heads that include all nodes that are not in
        # the badnodes set.
        return {
            ctx.hex()
            for ctx in repo.set("heads((draft() & ::%ls) - %ls::)", heads, badnodes)
        }
    else:
        return heads


def _dobackup(ui, repo, dest, **opts):
    ui.status(_("starting backup %s\n") % time.strftime("%H:%M:%S %d %b %Y %Z"))
    start = time.time()
    # to handle multiple working copies correctly
    currentbkpgenerationvalue = _readbackupgenerationfile(repo.sharedvfs)
    newbkpgenerationvalue = ui.configint("infinitepushbackup", "backupgeneration", 0)
    if currentbkpgenerationvalue != newbkpgenerationvalue:
        # Unlinking local backup state will trigger re-backuping
        _deletebackupstate(repo)
        _writebackupgenerationfile(repo.sharedvfs, newbkpgenerationvalue)
    bkpstate = _readlocalbackupstate(ui, repo)

    # Work out what the heads and bookmarks to backup are.
    headstobackup = _backupheads(ui, repo)
    localbookmarks = _getlocalbookmarks(repo)

    # We don't want to backup commits that are marked as bad.
    headstobackup = _filterbadnodes(ui, repo, headstobackup)

    # Remove heads that are no longer backup heads.
    backedupheads = set(bkpstate.heads)
    removedheads = backedupheads - headstobackup

    # We don't need to backup heads that have already been backed up.
    headstobackup -= backedupheads

    if (
        (bkpstate.empty() or localbookmarks == bkpstate.localbookmarks)
        and not headstobackup
        and not localbookmarks
    ):
        # There is nothing to backup, and either no previous backup state, or
        # the local bookmarks match the backed up ones.  Exit now to save
        # a connection to the server and to prevent accidentally clearing a
        # previous backup from the same location.
        ui.status(_("nothing to backup\n"))
        return

    # Push bundles for all of the commits, one stack at a time.
    path = _getremotepath(repo, ui, dest)

    def getconnection():
        return repo.connectionpool.get(path, opts)

    newheads, failedheads = infinitepush.pushbackupbundlestacks(
        ui, repo, getconnection, headstobackup
    )

    for head in failedheads:
        # We failed to push this head.  Don't remove any backup heads that
        # are ancestors of this head.
        removedheads -= set(ctx.hex() for ctx in repo.set("draft() & ::%s" % head))

    # Work out what bookmarks we are going to back up.
    backupbookmarks = _filterbookmarks(
        localbookmarks, repo, set(n for n in newheads | backedupheads if n in repo)
    )
    newbookmarks = _dictdiff(backupbookmarks, bkpstate.localbookmarks)
    removedbookmarks = _dictdiff(bkpstate.localbookmarks, backupbookmarks)

    namingmgr = BackupBookmarkNamingManager(ui, repo)
    infinitepushbookmarks = _getinfinitepushbookmarks(
        repo, newbookmarks, removedbookmarks, newheads, removedheads, namingmgr
    )

    # If the previous backup state was empty, we should clear the server's
    # view of the previous backup.
    if bkpstate.empty():
        infinitepushbookmarks[namingmgr.getbackupheadprefix()] = ""
        infinitepushbookmarks[namingmgr.getbackupbookmarkprefix()] = ""

    try:
        with getconnection() as conn:
            if infinitepush.pushbackupbundle(
                ui, repo, conn.peer, [], infinitepushbookmarks
            ):
                ui.debug("backup complete\n")
                ui.debug("heads added: %s\n" % ", ".join(newheads))
                ui.debug("heads removed: %s\n" % ", ".join(removedheads))
                _writelocalbackupstate(
                    repo.sharedvfs,
                    list((set(bkpstate.heads) | newheads) - removedheads),
                    backupbookmarks,
                )
            else:
                ui.status(_("nothing to backup\n"))
    finally:
        ui.status(_("finished in %.2f seconds\n") % (time.time() - start))
    if failedheads:
        raise error.Abort(_("failed to backup %d heads\n") % len(failedheads))


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


_backupstatefile = "infinitepushbackupstate"
_backupgenerationfile = "infinitepushbackupgeneration"

# Common helper functions
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
        reporoot = self.repo.sharedroot

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


def _getremotepath(repo, ui, dest):
    path = ui.paths.getpath(dest, default=("infinitepush", "default"))
    if not path:
        raise error.Abort(
            _("default repository not configured!"),
            hint=_("see 'hg help config.paths'"),
        )
    dest = path.pushloc or path.loc
    return dest


def _getremote(repo, ui, dest, **opts):
    dest = _getremotepath(repo, ui, dest)
    return hg.peer(repo, opts, dest)


def _getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts


# Backup helper functions


def _getinfinitepushbookmarks(
    repo, newbookmarks, removedbookmarks, newheads, removedheads, namingmgr
):
    infinitepushbookmarks = {}

    for bookmark, hexnode in removedbookmarks.items():
        backupbookmark = namingmgr.getbackupbookmarkname(bookmark)
        infinitepushbookmarks[backupbookmark] = ""

    for bookmark, hexnode in newbookmarks.items():
        backupbookmark = namingmgr.getbackupbookmarkname(bookmark)
        infinitepushbookmarks[backupbookmark] = hexnode

    for hexhead in removedheads:
        headbookmarksname = namingmgr.getbackupheadname(hexhead)
        infinitepushbookmarks[headbookmarksname] = ""

    for hexhead in newheads:
        headbookmarksname = namingmgr.getbackupheadname(hexhead)
        infinitepushbookmarks[headbookmarksname] = hexhead

    return infinitepushbookmarks


def _localbackupstateexists(repo):
    return repo.sharedvfs.exists(_backupstatefile)


def _deletebackupstate(repo):
    return repo.sharedvfs.tryunlink(_backupstatefile)


def _readlocalbackupstate(ui, repo):
    if not _localbackupstateexists(repo):
        return backupstate()

    with repo.sharedvfs(_backupstatefile) as f:
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
