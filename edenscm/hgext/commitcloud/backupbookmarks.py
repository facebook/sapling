# Copyright 2017-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib
import json
import os
import re
import socket

from edenscm.mercurial import encoding, error, node as nodemod, perftrace, phases, util
from edenscm.mercurial.i18n import _

from . import dependencies


prefix = "infinitepushbackups/infinitepushbackupstate"


def _escapebookmark(bookmark):
    """
    If ``bookmark`` contains "bookmarks" as a substring then replace it with
    "bookmarksbookmarks".

    Also, encode * since it is used for prefix pattern matching

    This is intended to make parsing bookmark names unambiguous, however it
    still has bugs.
    """
    bookmark = encoding.fromlocal(bookmark)
    bookmark = bookmark.replace("*", "*%")
    return bookmark.replace("bookmarks", "bookmarksbookmarks")


def _unescapebookmark(bookmark):
    bookmark = encoding.tolocal(bookmark)
    bookmark = bookmark.replace("*%", "*")
    return bookmark.replace("bookmarksbookmarks", "bookmarks")


def backuphostname(repo):
    hostname = repo.ui.config("infinitepushbackup", "hostname")
    if not hostname:
        hostname = socket.gethostname()
    return hostname


def _backupbookmarkprefix(repo, username=None, hostname=None, reporoot=None):
    """returns the backup bookmark prefix for this user and repo

    The naming convention is:
    - infinitepush/backups/<username>/<hostname>/<reporoot>/bookmarks/<name>
    - infinitepush/backups/<username>/<hostname>/<reporoot>/heads/<hash>

    This function returns everything up to just after the reporoot.
    """
    if not username:
        username = util.shortuser(repo.ui.username())
    if not hostname:
        hostname = backuphostname(repo)
    if not reporoot:
        reporoot = repo.sharedroot
    reporoot = reporoot.strip("/")
    return "/".join(("infinitepush", "backups", username, hostname, reporoot))


_backupstateprefix = "infinitepushbackups/infinitepushbackupstate"


def _localbackupstatepath(remotepath):
    hash = hashlib.sha256(remotepath).hexdigest()[0:8]
    return os.path.join(_backupstateprefix + "_" + hash)


def _localbackupstateexists(repo, remotepath):
    return repo.sharedvfs.exists(_localbackupstatepath(remotepath))


def _deletebackupstate(repo, remotepath):
    return repo.sharedvfs.tryunlink(_localbackupstatepath(remotepath))


def _readlocalbackupstate(repo, remotepath, doingbackup=False):
    if _localbackupstateexists(repo, remotepath):
        backupstatefile = _localbackupstatepath(remotepath)
        with repo.sharedvfs(backupstatefile) as f:
            try:
                state = json.loads(f.read())
                if not isinstance(state["bookmarks"], dict) or not isinstance(
                    state["heads"], list
                ):
                    raise ValueError("bad types of bookmarks or heads")

                heads = set(map(str, state["heads"]))
                bookmarks = state["bookmarks"]
                return heads, bookmarks
            except (ValueError, KeyError, TypeError) as e:
                repo.ui.warn(_("corrupt file: %s (%s)\n") % (backupstatefile, e))
    return None


def _writelocalbackupstate(repo, remotepath, heads, bookmarks):
    state = {"heads": list(heads), "bookmarks": bookmarks, "remotepath": remotepath}
    with repo.sharedvfs(_localbackupstatepath(remotepath), "w", atomictemp=True) as f:
        json.dump(state, f)


@perftrace.tracefunc("Push Commit Cloud Backup Bookmarks")
def pushbackupbookmarks(repo, remotepath, getconnection, backupstate):
    """
    Push a backup bundle to the server that updates the infinitepush backup
    bookmarks.
    """
    unfi = repo.unfiltered()

    # Create backup bookmarks for the heads and bookmarks of the user.  We
    # need to include only commit that have been successfully backed up, so
    # that we can sure they are available on the server.
    clrev = unfi.changelog.rev
    ancestors = unfi.changelog.ancestors(
        [clrev(head) for head in backupstate.heads], inclusive=True
    )
    # Get the heads of visible draft commits that are already backed up,
    # including commits made visible by bookmarks.
    #
    # For historical compatibility, we ignore obsolete and secret commits
    # as they are normally excluded from backup bookmarks.
    with perftrace.trace("Compute Heads"):
        revset = "heads((draft() & ::((draft() - obsolete() - hidden()) + bookmark())) & (draft() & ::%ln))"
        heads = [nodemod.hex(head) for head in unfi.nodes(revset, backupstate.heads)]
    # Get the bookmarks that point to ancestors of backed up draft commits or
    # to commits that are public.
    with perftrace.trace("Compute Bookmarks"):
        bookmarks = {}
        for name, node in repo._bookmarks.iteritems():
            ctx = repo[node]
            if ctx.rev() in ancestors or ctx.phase() == phases.public:
                bookmarks[name] = ctx.hex()

    infinitepushbookmarks = {}
    prefix = _backupbookmarkprefix(repo)
    localstate = _readlocalbackupstate(repo, remotepath)

    if localstate is None:
        # If there is nothing to backup, don't push any backup bookmarks yet.
        # The user may wish to restore the previous backup.
        if not heads and not bookmarks:
            return

        # Delete all server bookmarks and replace them with the full set.  The
        # server knows to do deletes before adds, and deletes are done by glob
        # pattern (see infinitepush.bundleparts.bundle2scratchbookmarks).
        infinitepushbookmarks["/".join((prefix, "heads", "*"))] = ""
        infinitepushbookmarks["/".join((prefix, "bookmarks", "*"))] = ""
        oldheads = set()
        oldbookmarks = {}
    else:
        # Generate a delta update based on the local state.
        oldheads, oldbookmarks = localstate

        if set(oldheads) == set(heads) and oldbookmarks == bookmarks:
            return

        for oldhead in oldheads:
            if oldhead not in heads:
                infinitepushbookmarks["/".join((prefix, "heads", oldhead))] = ""
        for oldbookmark in oldbookmarks:
            if oldbookmark not in bookmarks:
                infinitepushbookmarks[
                    "/".join((prefix, "bookmarks", _escapebookmark(oldbookmark)))
                ] = ""

    for bookmark, hexnode in bookmarks.items():
        if bookmark not in oldbookmarks or hexnode != oldbookmarks[bookmark]:
            name = "/".join((prefix, "bookmarks", _escapebookmark(bookmark)))
            infinitepushbookmarks[name] = hexnode
    for hexhead in heads:
        if hexhead not in oldheads:
            name = "/".join((prefix, "heads", hexhead))
            infinitepushbookmarks[name] = hexhead

    if not infinitepushbookmarks:
        return

    # developer config: infinitepushbackup.backupbookmarklimit
    backupbookmarklimit = repo.ui.configint(
        "infinitepushbackup", "backupbookmarklimit", 1000
    )
    if len(infinitepushbookmarks) > backupbookmarklimit:
        repo.ui.warn(
            _("not pushing backup bookmarks for %s as there are too many (%s > %s)\n")
            % (prefix, len(infinitepushbookmarks), backupbookmarklimit),
            notice=_("warning"),
            component="commitcloud",
        )
        return

    # Push a bundle containing the new bookmarks to the server.
    with perftrace.trace("Push Backup Bookmark Bundle"), getconnection() as conn:
        dependencies.infinitepush.pushbackupbundle(
            repo.ui, repo, conn.peer, None, infinitepushbookmarks
        )

    # Store the new local backup state.
    _writelocalbackupstate(repo, remotepath, heads, bookmarks)


_backupbookmarkre = re.compile(
    "^infinitepush/backups/([^/]*)/([^/]*)(/.*)/(bookmarks|heads)/(.*)$"
)


def downloadbackupbookmarks(
    repo,
    remotepath,
    getconnection,
    sourceusername,
    sourcehostname=None,
    sourcereporoot=None,
):
    """download backup bookmarks from the server

    Returns an ordered dict mapping:
      (hostname, reporoot) => {"heads": [NODE, ...], "bookmarks": {NAME: NODE, ...}}

    Sqlindex returns backups in order of insertion.  Hostnames and reporoot in
    the dict should be in most-recently-used order, so the fresher backups come
    first. Within the backups, the order of insertion is preserved.

    Fileindex returns backups in lexicographic order, since the fileindex
    doesn't support maintaining the order of insertion.
    """

    pattern = "infinitepush/backups/%s" % sourceusername
    if sourcehostname:
        pattern += "/%s" % sourcehostname
        if sourcereporoot:
            pattern += sourcereporoot
    pattern += "/*"

    with getconnection() as conn:
        if "listkeyspatterns" not in conn.peer.capabilities():
            raise error.Abort(
                "'listkeyspatterns' command is not supported for the server %s"
                % conn.peer.url()
            )
        bookmarks = conn.peer.listkeyspatterns("bookmarks", patterns=[pattern])

    backupinfo = util.sortdict()
    for name, hexnode in bookmarks.iteritems():

        match = _backupbookmarkre.match(name)
        if match:
            username, hostname, reporoot, type, name = match.groups()

            if sourcereporoot and sourcereporoot != reporoot:
                continue
            if sourcehostname and sourcehostname != hostname:
                continue
            entry = backupinfo.setdefault((hostname, reporoot), {})
            if type == "heads":
                entry.setdefault("heads", []).append(hexnode)
            elif type == "bookmarks":
                entry.setdefault("bookmarks", {})[_unescapebookmark(name)] = hexnode
        else:
            repo.ui.warn(
                _("backup bookmark format unrecognised: '%s' -> %s") % (name, hexnode)
            )

    # Reverse to make MRU order
    backupinfomru = util.sortdict()
    for key, value in reversed(backupinfo.items()):
        backupinfomru[key] = value

    return backupinfomru


def printbackupbookmarks(ui, username, backupbookmarks, all=False):
    ui.write(
        _(
            "user %s has %d available backups:\n"
            "(backups are ordered with the most recent at the top of the list)\n"
        )
        % (username, len(backupbookmarks))
    )

    limit = ui.configint("infinitepushbackup", "backuplistlimit")
    for i, (hostname, reporoot) in enumerate(backupbookmarks.keys()):
        if not all and i == limit:
            ui.write(
                _(
                    "(older backups have been hidden, "
                    "run 'hg cloud listbackups --all' to see them all)\n"
                )
            )
            break
        ui.write(_("%s on %s\n") % (reporoot, hostname))


def deletebackupbookmarks(
    repo, remotepath, getconnection, targetusername, targethostname, targetreporoot
):
    prefix = _backupbookmarkprefix(repo, targetusername, targethostname, targetreporoot)

    # If we're deleting the bookmarks for the local repo, also delete its
    # state.
    if prefix == _backupbookmarkprefix(repo):
        _deletebackupstate(repo, remotepath)

    # Push a bundle containing the new bookmarks to the server.
    infinitepushbookmarks = {}
    infinitepushbookmarks["/".join((prefix, "heads", "*"))] = ""
    infinitepushbookmarks["/".join((prefix, "bookmarks", "*"))] = ""
    with getconnection() as conn:
        dependencies.infinitepush.pushbackupbundle(
            repo.ui, repo, conn.peer, None, infinitepushbookmarks
        )
