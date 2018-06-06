# hgsql.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# no-check-code
# flake8: noqa

from __future__ import absolute_import

import os
import Queue
import sys
import threading
import time
import warnings

from mercurial import (
    bookmarks,
    bundle2,
    bundlerepo,
    changegroup,
    commands,
    demandimport,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    mdiff,
    phases,
    progress,
    registrar,
    repair,
    revlog,
    util,
    wireproto,
)
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid, nullrev


wrapcommand = extensions.wrapcommand
wrapfunction = extensions.wrapfunction

# mysql.connector does not import nicely with the demandimporter, so temporarily
# disable it.
try:
    xrange(0)
except NameError:
    xrange = range

with demandimport.deactivated():
    import mysql.connector

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "3.9.1"

configtable = {}
configitem = registrar.configitem(configtable)

configitem("hgsql", "database", default=None)
configitem("hgsql", "host", default=None)
configitem("hgsql", "reponame", default=None)
configitem("hgsql", "password", default="")
configitem("hgsql", "port", default=0)
configitem("hgsql", "user", default=None)

configitem("hgsql", "bypass", default=False)
configitem("hgsql", "enabled", default=False)
configitem("hgsql", "engine", default="innodb")
configitem("hgsql", "locktimeout", default=60)
configitem("hgsql", "maxcommitsize", default=52428800)
configitem("hgsql", "maxinsertsize", default=1048576)
configitem("hgsql", "maxrowsize", default=1048576)
configitem("hgsql", "profileoutput", default="/tmp")
configitem("hgsql", "profiler", default=None)
configitem("hgsql", "verifybatchsize", default=1000)
configitem("hgsql", "waittimeout", default=300)
configitem("format", "usehgsql", default=True)

writelock = "write_lock"

INITIAL_SYNC_NORMAL = "normal"
INITIAL_SYNC_DISABLE = "disabled"
INITIAL_SYNC_FORCE = "force"

initialsync = INITIAL_SYNC_NORMAL

cls = localrepo.localrepository
# Do NOT add hgsql to localrepository.supportedformats. Doing that breaks
# streaming clones.
for reqs in ["openerreqs", "_basesupported"]:
    getattr(cls, reqs).add("hgsql")


def newreporequirements(orig, repo):
    reqs = orig(repo)
    if repo.ui.configbool("format", "usehgsql"):
        reqs.add("hgsql")
    return reqs


class CorruptionException(Exception):
    pass


def issqlrepo(repo):
    return repo.ui.configbool("hgsql", "enabled")


def cansyncwithsql(repo):
    return issqlrepo(repo) and not isinstance(repo, bundlerepo.bundlerepository)


def uisetup(ui):
    if ui.configbool("hgsql", "bypass"):
        return
    wrapfunction(localrepo, "newreporequirements", newreporequirements)

    # Enable SQL for local commands that write to the repository.
    wrapcommand(commands.table, "pull", pull)
    wrapcommand(commands.table, "commit", commit)

    wrapcommand(commands.table, "bookmark", bookmarkcommand)
    wrapfunction(exchange, "_localphasemove", _localphasemove)
    wrapfunction(exchange.pulloperation, "__init__", pullop_init)
    wrapfunction(exchange, "push", push)

    # Enable SQL for remote commands that write to the repository
    wireproto.commands["unbundle"] = (wireproto.unbundle, "heads")
    wrapfunction(exchange, "unbundle", unbundle)

    wrapfunction(wireproto, "pushkey", pushkey)
    wireproto.commands["pushkey"] = (wireproto.pushkey, "namespace key old new")

    wrapfunction(bookmarks, "updatefromremote", updatefromremote)
    if util.safehasattr(changegroup, "addchangegroup"):
        wrapfunction(changegroup, "addchangegroup", addchangegroup)
    else:
        # Mercurial 3.6+
        wrapfunction(changegroup.cg1unpacker, "apply", changegroupapply)

    try:
        treemfmod = extensions.find("treemanifest")
        wrapcommand(treemfmod.cmdtable, "backfilltree", backfilltree)
    except KeyError:
        pass

    # Record revlog writes
    def writeentry(orig, self, transaction, ifh, dfh, entry, data, link, offset):
        """records each revlog write to the repo's pendingrev list"""
        if not util.safehasattr(transaction, "repo"):
            return orig(self, transaction, ifh, dfh, entry, data, link, offset)

        e = revlog.indexformatng.unpack(entry)
        node = hex(e[7])
        data0 = data[0] or ""
        transaction.repo.pendingrevs.append(
            (self.indexfile, link, len(self) - 1, node, entry, data0, data[1])
        )
        return orig(self, transaction, ifh, dfh, entry, data, link, offset)

    wrapfunction(revlog.revlog, "_writeentry", writeentry)

    # Reorder incoming revs to be in linkrev order
    wrapfunction(revlog.revlog, "addgroup", addgroup)


def extsetup(ui):
    if ui.configbool("hgsql", "bypass"):
        return

    if ui.configbool("hgsql", "enabled"):
        commands.globalopts.append(
            (
                "",
                "forcesync",
                False,
                _("force hgsql sync even on read-only commands"),
                _("TYPE"),
            )
        )

    # Directly examining argv seems like a terrible idea, but it seems
    # neccesary unless we refactor mercurial dispatch code. This is because
    # the first place we have access to parsed options is in the same function
    # (dispatch.dispatch) that created the repo and the repo creation initiates
    # the sync operation in which the lock is elided unless we set this.
    if "--forcesync" in sys.argv:
        ui.debug("forcesync enabled\n")
        global initialsync
        initialsync = INITIAL_SYNC_FORCE


def reposetup(ui, repo):
    if ui.configbool("hgsql", "bypass"):
        return

    if issqlrepo(repo):
        wraprepo(repo)

        if initialsync != INITIAL_SYNC_DISABLE and cansyncwithsql(repo):
            # Use a noop to force a sync
            def noop():
                pass

            waitforlock = initialsync == INITIAL_SYNC_FORCE
            executewithsql(repo, noop, waitforlock=waitforlock)


# Incoming commits are only allowed via push and pull
def unbundle(orig, repo, cg, *args, **kwargs):
    if not issqlrepo(repo):
        return orig(repo, cg, *args, **kwargs)

    isbundle2 = util.safehasattr(cg, "params")
    islazylocking = repo.ui.configbool("experimental", "bundle2lazylocking")
    if isbundle2 and islazylocking:
        # lazy locked bundle2
        exception = None
        oldopclass = None
        context = None
        try:
            context = sqlcontext(repo, takelock=True, waitforlock=True)

            # Temporarily replace bundleoperation so we can hook into it's
            # locking mechanism.
            oldopclass = bundle2.bundleoperation

            class sqllockedoperation(bundle2.bundleoperation):
                def __init__(self, repo, transactiongetter, *args, **kwargs):
                    def sqllocktr():
                        if not context.active():
                            context.__enter__()
                        return transactiongetter()

                    super(sqllockedoperation, self).__init__(
                        repo, sqllocktr, *args, **kwargs
                    )
                    # undo our temporary wrapping
                    bundle2.bundleoperation = oldopclass

            bundle2.bundleoperation = sqllockedoperation

            return orig(repo, cg, *args, **kwargs)
        except Exception as ex:
            exception = ex
            raise
        finally:
            # Be extra sure to undo our wrapping
            if oldopclass:
                bundle2.bundleoperation = oldopclass
            # release sqllock here in exit
            if context:
                type = value = traceback = None
                if exception:
                    type = exception.__class__
                    value = exception
                    traceback = []  # This isn't really important
                context.__exit__(type, value, traceback)
    else:
        # bundle1 or non-lazy locked
        return executewithsql(repo, orig, True, repo, cg, *args, **kwargs)


def pull(orig, *args, **kwargs):
    repo = args[1]
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


def pullop_init(orig, self, repo, *args, **kwargs):
    if issqlrepo(repo) or "hgsql" in repo.requirements:
        kwargs["exactbyteclone"] = True
    return orig(self, repo, *args, **kwargs)


def push(orig, *args, **kwargs):
    repo = args[0]
    if issqlrepo(repo):
        # A push locks the local repo in order to update phase data, so we need
        # to take the lock for the local repo during a push.
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


def commit(orig, *args, **kwargs):
    repo = args[1]
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


def updatefromremote(orig, *args, **kwargs):
    repo = args[1]
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


def addchangegroup(orig, *args, **kwargs):
    repo = args[0]
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


def backfilltree(orig, ui, repo, *args, **kwargs):
    if issqlrepo(repo):

        def _helper():
            return orig(ui, repo, *args, **kwargs)

        try:
            repo.sqlreplaytransaction = True
            return executewithsql(repo, _helper, True)
        finally:
            repo.sqlreplaytransaction = False
    else:
        return orig(ui, repo, *args, **kwargs)


def changegroupapply(orig, *args, **kwargs):
    repo = args[1]
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


def _localphasemove(orig, pushop, *args, **kwargs):
    repo = pushop.repo
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, pushop, *args, **kwargs)
    else:
        return orig(pushop, *args, **kwargs)


class sqlcontext(object):
    def __init__(self, repo, takelock=False, waitforlock=False):
        self.repo = repo
        self.takelock = takelock
        self.waitforlock = waitforlock
        self._connected = False
        self._locked = False
        self._used = False
        self._startlocktime = 0
        self._active = False
        self._profiler = None

    def active(self):
        return self._active

    def __enter__(self):
        if self._used:
            raise Exception("error: using sqlcontext twice")
        self._used = True
        self._active = True

        repo = self.repo
        if not repo.sqlconn:
            repo.sqlconnect()
            self._connected = True

        if self.takelock and not writelock in repo.heldlocks:
            startwait = time.time()
            try:
                repo.sqllock(writelock)
            except error.Abort:
                elapsed = time.time() - startwait
                repo.ui.log(
                    "sqllock",
                    "failed to get sql lock after %s " "seconds\n",
                    elapsed,
                    elapsed=elapsed * 1000,
                    valuetype="lockwait",
                    success="false",
                    repository=repo.root,
                )
                raise

            self._startprofile()
            self._locked = True
            elapsed = time.time() - startwait
            repo.ui.log(
                "sqllock",
                "waited for sql lock for %s seconds\n",
                elapsed,
                elapsed=elapsed * 1000,
                valuetype="lockwait",
                success="true",
                repository=repo.root,
            )
        self._startlocktime = time.time()

        if self._connected:
            repo.syncdb(waitforlock=self.waitforlock)

    def __exit__(self, type, value, traceback):
        try:
            repo = self.repo
            if self._locked:
                elapsed = time.time() - self._startlocktime
                repo.ui.log(
                    "sqllock",
                    "held sql lock for %s seconds\n",
                    elapsed,
                    elapsed=elapsed * 1000,
                    valuetype="lockheld",
                    repository=repo.root,
                )
                self._stopprofile(elapsed)
                repo.sqlunlock(writelock)

            if self._connected:
                repo.sqlclose()

            self._active = False
        except mysql.connector.errors.Error:
            # Only raise sql exceptions if the wrapped code threw no exception
            if type is None:
                raise

    def _startprofile(self):
        profiler = self.repo.ui.config("hgsql", "profiler")
        if not profiler:
            return

        freq = self.repo.ui.configint("profiling", "freq")
        if profiler == "ls":
            from mercurial import lsprof

            self._profiler = lsprof.Profiler()
            self._profiler.enable(subcalls=True)
        elif profiler == "stat":
            from mercurial import statprof

            statprof.reset(freq)
            statprof.start()
        else:
            raise Exception("unknown profiler: %s" % profiler)

    def _stopprofile(self, elapsed):
        profiler = self.repo.ui.config("hgsql", "profiler")
        if not profiler:
            return
        outputdir = self.repo.ui.config("hgsql", "profileoutput")
        import random

        pid = os.getpid()
        rand = random.random()
        timestamp = time.time()

        if profiler == "ls":
            from mercurial import lsprof

            self._profiler.disable()
            stats = lsprof.Stats(self._profiler.getstats())
            stats.sort("inlinetime")
            path = os.path.join(
                outputdir, "hgsql-profile-%s-%s-%s" % (pid, timestamp, rand)
            )
            with open(path, "a+") as f:
                stats.pprint(limit=30, file=f, climit=0)
                f.write("Total Elapsed Time: %s\n" % elapsed)
        elif profiler == "stat":
            from mercurial import statprof

            statprof.stop()
            path = os.path.join(
                outputdir, "hgsql-profile-%s-%s-%s" % (pid, timestamp, rand)
            )
            with open(path, "a+") as f:
                statprof.display(f)
                f.write("Total Elapsed Time: %s\n" % elapsed)


def executewithsql(repo, action, sqllock=False, *args, **kwargs):
    """Executes the given action while having a SQL connection open.
    If a locks are specified, those locks are held for the duration of the
    action.
    """
    # executewithsql can be executed in a nested scenario (ex: writing
    # bookmarks during a pull), so track whether this call performed
    # the connect.

    waitforlock = sqllock
    if "waitforlock" in kwargs:
        if not waitforlock:
            waitforlock = kwargs["waitforlock"]
        del kwargs["waitforlock"]

    with sqlcontext(repo, takelock=sqllock, waitforlock=waitforlock):
        return action(*args, **kwargs)


def wraprepo(repo):
    class sqllocalrepo(repo.__class__):
        def sqlconnect(self):
            if self.sqlconn:
                raise Exception("SQL connection already open")
            if self.sqlcursor:
                raise Exception("SQL cursor already open without connection")
            retry = 3
            while True:
                try:
                    self.sqlconn = mysql.connector.connect(
                        force_ipv6=True, **self.sqlargs
                    )

                    # The default behavior is to return byte arrays, when we
                    # need strings. This custom convert returns strings.
                    self.sqlconn.set_converter_class(CustomConverter)
                    break
                except mysql.connector.errors.Error:
                    # mysql can be flakey occasionally, so do some minimal
                    # retrying.
                    retry -= 1
                    if retry == 0:
                        raise
                    time.sleep(0.2)

            waittimeout = self.ui.config("hgsql", "waittimeout")
            waittimeout = self.sqlconn.converter.escape("%s" % waittimeout)

            self.engine = self.ui.config("hgsql", "engine")
            self.locktimeout = self.ui.config("hgsql", "locktimeout")
            self.locktimeout = self.sqlconn.converter.escape("%s" % self.locktimeout)

            self.sqlcursor = self.sqlconn.cursor()
            self.sqlcursor.execute("SET wait_timeout=%s" % waittimeout)

            if self.engine == "rocksdb":
                self.sqlcursor.execute(
                    "SET rocksdb_lock_wait_timeout=%s" % self.locktimeout
                )
            elif self.engine == "innodb":
                self.sqlcursor.execute(
                    "SET innodb_lock_wait_timeout=%s" % self.locktimeout
                )
            else:
                raise RuntimeError("unsupported hgsql.engine %s" % self.engine)

        def sqlclose(self):
            with warnings.catch_warnings():
                warnings.simplefilter("ignore")
                self.sqlcursor.close()
                self.sqlconn.close()
            self.sqlcursor = None
            self.sqlconn = None

        def _lockname(self, name):
            lockname = "%s_%s" % (name, self.sqlreponame)
            return self.sqlconn.converter.escape(lockname)

        def sqllock(self, name):
            lockname = self._lockname(name)

            # cast to int to prevent passing bad sql data
            self.sqlcursor.execute(
                "SELECT GET_LOCK('%s', %s)" % (lockname, self.locktimeout)
            )

            result = int(self.sqlcursor.fetchall()[0][0])
            if result != 1:
                raise util.Abort(
                    "timed out waiting for mysql repo lock (%s)" % lockname
                )
            self.heldlocks.add(name)

        def hassqllock(self, name, checkserver=True):
            if not name in self.heldlocks:
                return False

            if not checkserver:
                return True

            lockname = self._lockname(name)
            self.sqlcursor.execute("SELECT IS_USED_LOCK('%s')" % (lockname,))
            lockheldby = self.sqlcursor.fetchall()[0][0]
            if lockheldby == None:
                raise Exception("unable to check %s lock" % lockname)

            self.sqlcursor.execute("SELECT CONNECTION_ID()")
            myconnectid = self.sqlcursor.fetchall()[0][0]
            if myconnectid == None:
                raise Exception("unable to read connection id")

            return lockheldby == myconnectid

        def sqlunlock(self, name):
            lockname = self._lockname(name)
            self.sqlcursor.execute("SELECT RELEASE_LOCK('%s')" % (lockname,))
            self.sqlcursor.fetchall()
            self.heldlocks.discard(name)

            for callback in self.sqlpostrelease:
                callback()
            self.sqlpostrelease = []

        def lock(self, *args, **kwargs):
            wl = self._wlockref and self._wlockref()
            if (
                not self._issyncing
                and not (wl is not None and wl.held)
                and not self.hassqllock(writelock, checkserver=False)
            ):
                self._recordbadlockorder()
            return super(sqllocalrepo, self).lock(*args, **kwargs)

        def wlock(self, *args, **kwargs):
            if not self._issyncing and not self.hassqllock(
                writelock, checkserver=False
            ):
                self._recordbadlockorder()
            return super(sqllocalrepo, self).wlock(*args, **kwargs)

        def _recordbadlockorder(self):
            self.ui.debug("hgsql: invalid lock order\n")

        def transaction(self, *args, **kwargs):
            tr = super(sqllocalrepo, self).transaction(*args, **kwargs)
            if tr.count > 1:
                return tr

            validator = tr.validator

            def pretxnclose(tr):
                validator(tr)
                self.committodb(tr)
                del self.pendingrevs[:]

            tr.validator = pretxnclose

            def transactionabort(orig):
                del self.pendingrevs[:]
                return orig()

            wrapfunction(tr, "_abort", transactionabort)

            tr.repo = self
            return tr

        def needsync(self):
            """Returns True if the local repo is not in sync with the database.
            If it returns False, the heads and bookmarks match the database.
            """
            self.sqlcursor.execute(
                """SELECT namespace, name, value
                FROM revision_references WHERE repo = %s""",
                (self.sqlreponame,),
            )
            sqlheads = set()
            sqlbookmarks = {}
            tip = -1
            for namespace, name, value in self.sqlcursor:
                if namespace == "heads":
                    sqlheads.add(bin(value))
                elif namespace == "bookmarks":
                    sqlbookmarks[name] = bin(value)
                elif namespace == "tip":
                    tip = int(value)

            # Since we don't have the lock right now, and since this is the
            # first place we load the changelog and bookmarks off disk, it's
            # important that we load bookmarks before the changelog here. This
            # way we know that the bookmarks point to valid nodes. Otherwise,
            # the bookmarks might change between us reading the changelog and
            # the bookmark file.
            bookmarks = self._bookmarks
            heads = set(self.heads())

            outofsync = (
                heads != sqlheads or bookmarks != sqlbookmarks or tip != len(self) - 1
            )
            return outofsync, sqlheads, sqlbookmarks, tip

        def synclimiter(self):
            """Attempts to acquire the lock used to rate limit how many
            read-only clients perform database syncs at the same time. If None
            is returned, it means the limiter was not acquired, and readonly
            clients should not attempt to perform a sync."""
            try:
                wait = False
                return self._lock(
                    self.svfs,
                    "synclimiter",
                    wait,
                    None,
                    None,
                    _("repository %s") % self.origroot,
                )
            except error.LockHeld:
                return None

        def syncdb(self, waitforlock=False):
            """Attempts to sync the local repository with the latest bits in the
            database.

            If `waitforlock` is False, the sync is on a best effort basis,
            and the repo may not actually be up-to-date afterwards. If
            `waitforlock` is True, we guarantee that the repo is up-to-date when
            this function returns, otherwise an exception will be thrown."""
            try:
                self._issyncing = True
                if waitforlock:
                    return self._syncdb(waitforlock)
                else:
                    # For operations that do not require the absolute latest bits,
                    # only let one process update the repo at a time.
                    limiter = self.synclimiter()
                    if not limiter:
                        # Someone else is already checking and updating the repo
                        self.ui.debug(
                            "skipping database sync because another "
                            "process is already syncing\n"
                        )

                        # It's important that we load bookmarks before the
                        # changelog. This way we know that the bookmarks point to
                        # valid nodes. Otherwise, the bookmarks might change between
                        # us reading the changelog and the bookmark file. Normally
                        # this would be done in needsync(), but since we're skipping
                        # the sync, we can do it here. Accessing self._bookmarks
                        # loads both the bookmarks and the changelog.
                        self._bookmarks
                        return

                    try:
                        return self._syncdb(waitforlock)
                    finally:
                        limiter.release()
            finally:
                self._issyncing = False

        def _syncdb(self, waitforlock):
            if not self.needsync()[0]:
                ui.debug("syncing not needed\n")
                return
            ui.debug("syncing with mysql\n")

            # Save a copy of the old manifest cache so we can put it back
            # afterwards.
            oldmancache = self.manifestlog._dirmancache

            wlock = lock = None
            try:
                wlock = self.wlock(wait=waitforlock)
                lock = self.lock(wait=waitforlock)
            except error.LockHeld:
                if waitforlock:
                    raise
                # Oh well. Don't block this non-critical read-only operation.
                ui.debug("skipping sync for current operation\n")
                return

            configbackups = []
            try:
                # Disable all pretxnclose hooks, since these revisions are
                # technically already commited.
                for name, value in ui.configitems("hooks"):
                    if name.startswith("pretxnclose"):
                        configbackups.append(ui.backupconfig("hooks", name))
                        ui.setconfig("hooks", name, None)
                # The hg-ssh wrapper installs a hook to block all writes. We need to
                # circumvent this when we sync from the server.
                configbackups.append(ui.backupconfig("hooks", "pretxnopen.hg-ssh"))
                self.ui.setconfig("hooks", "pretxnopen.hg-ssh", None)
                configbackups.append(ui.backupconfig("hooks", "pretxnopen.hg-rsh"))
                self.ui.setconfig("hooks", "pretxnopen.hg-rsh", None)
                configbackups.append(
                    ui.backupconfig("hooks", "pretxnopen.readonlyrejectpush")
                )
                self.ui.setconfig("hooks", "pretxnopen.readonlyrejectpush", None)
                # Someone else may have synced us while we were waiting.
                # Restart the transaction so we have access to the latest rows.
                self.sqlconn.rollback()
                outofsync, sqlheads, sqlbookmarks, fetchend = self.needsync()
                if not outofsync:
                    return

                transaction = self.transaction("syncdb")

                self.hook("presyncdb", throw=True)

                try:
                    # Inspect the changelog now that we have the lock
                    fetchstart = len(self.changelog)

                    queue = Queue.Queue()
                    abort = threading.Event()

                    t = threading.Thread(
                        target=self.fetchthread,
                        args=(queue, abort, fetchstart, fetchend),
                    )
                    t.setDaemon(True)
                    try:
                        t.start()
                        addentries(self, queue, transaction)
                    finally:
                        abort.set()

                    phases.advanceboundary(
                        self, transaction, phases.public, self.heads()
                    )

                    transaction.close()
                finally:
                    transaction.release()

                # We circumvent the changelog and manifest when we add entries to
                # the revlogs. So clear all the caches.
                self.invalidate()
                self._filecache.pop("changelog", None)
                self._filecache.pop("manifestlog", None)
                self._filecache.pop("_phasecache", None)

                # Refill the cache. We can't just reuse the exact contents of
                # the old cached ctx, since the old ctx contains a reference to
                # the old revlog, which is now out of date.
                mfl = self.manifestlog
                for dirname, lrucache in oldmancache.iteritems():
                    if dirname == "":
                        for oldmfnode in lrucache:
                            oldmfctx = lrucache[oldmfnode]
                            if oldmfctx._data is not None:
                                mfl[oldmfnode]._data = oldmfctx._data

                heads = set(self.heads())
                heads.discard(nullid)
                if heads != sqlheads:
                    raise CorruptionException("heads don't match after sync")

                if len(self) - 1 != fetchend:
                    raise CorruptionException("tip doesn't match after sync")

                self.disablesync = True
                transaction = self.transaction("syncdb_bookmarks")
                try:
                    bm = self._bookmarks

                    self.sqlcursor.execute(
                        """SELECT name, value FROM revision_references
                        WHERE namespace = 'bookmarks' AND repo = %s""",
                        (self.sqlreponame,),
                    )
                    fetchedbookmarks = self.sqlcursor.fetchall()

                    changes = []
                    for name, node in fetchedbookmarks:
                        node = bin(node)
                        if node != bm.get(name):
                            changes.append((name, node))

                    for deletebm in set(bm.keys()).difference(
                        k for k, v in fetchedbookmarks
                    ):
                        changes.append((deletebm, None))

                    bm.applychanges(self, transaction, changes)
                    transaction.close()
                finally:
                    transaction.release()
                    self.disablesync = False

                if bm != sqlbookmarks:
                    raise CorruptionException("bookmarks don't match after sync")
            finally:
                for backup in configbackups:
                    ui.restoreconfig(backup)
                if lock:
                    lock.release()
                if wlock:
                    wlock.release()

            # Since we just exited the lock, the changelog and bookmark
            # in-memory structures will need to be reloaded. If we loaded
            # changelog before bookmarks, we might accidentally load bookmarks
            # that don't exist in the loaded changelog. So let's force loading
            # bookmarks now.
            bm = self._bookmarks

        def fetchthread(self, queue, abort, fetchstart, fetchend):
            """Fetches every revision from fetchstart to fetchend (inclusive)
            and places them on the queue. This function is meant to run on a
            background thread and listens to the abort event to abort early.
            """
            clrev = fetchstart
            chunksize = 1000
            try:
                while True:
                    if abort.isSet():
                        break

                    maxrev = min(clrev + chunksize, fetchend + 1)
                    self.sqlcursor.execute(
                        """SELECT path, chunk, chunkcount,
                        linkrev, entry, data0, data1 FROM revisions WHERE repo = %s
                        AND linkrev > %s AND linkrev < %s ORDER BY linkrev ASC""",
                        (self.sqlreponame, clrev - 1, maxrev),
                    )

                    # Put split chunks back together into a single revision
                    groupedrevdata = {}
                    for revdata in self.sqlcursor:
                        name = revdata[0]
                        chunk = revdata[1]
                        linkrev = revdata[3]

                        # Some versions of the MySQL Python connector have a
                        # bug where it converts aa column containing a single
                        # null byte into None. hgsql needs this workaround to
                        # handle file revisions that are exactly a single null
                        # byte.
                        #
                        # The only column that can contain a single null byte
                        # here is data1 (column 6):
                        # * path is a path, so Unix rules prohibit it from
                        #    containing null bytes.
                        # * chunk, chunkcount and linkrev are integers.
                        # * entry is a binary blob that matches a revlog index
                        #   entry, which cannot be "\0".
                        # * data0 is either empty or "u".
                        if revdata[6] is None:
                            revdata = revdata[:6] + (b"\0",)

                        groupedrevdata.setdefault((name, linkrev), {})[chunk] = revdata

                    if not groupedrevdata:
                        break

                    fullrevisions = []
                    for chunks in groupedrevdata.itervalues():
                        chunkcount = chunks[0][2]
                        if chunkcount == 1:
                            fullrevisions.append(chunks[0])
                        elif chunkcount == len(chunks):
                            fullchunk = list(chunks[0])
                            data1 = ""
                            for i in range(0, chunkcount):
                                data1 += chunks[i][6]
                            fullchunk[6] = data1
                            fullrevisions.append(tuple(fullchunk))
                        else:
                            raise Exception(
                                "missing revision chunk - expected %s got %s"
                                % (chunkcount, len(chunks))
                            )

                    fullrevisions = sorted(
                        fullrevisions, key=lambda revdata: revdata[3]
                    )
                    for revdata in fullrevisions:
                        queue.put(revdata)

                    clrev += chunksize
            except Exception as ex:
                queue.put(ex)
                return

            queue.put(False)

        def pushkey(self, namespace, key, old, new):
            def _pushkey():
                return super(sqllocalrepo, self).pushkey(namespace, key, old, new)

            return executewithsql(self, _pushkey, namespace == "bookmarks")

        def committodb(self, tr):
            """Commits all pending revisions to the database
            """
            if self.disablesync:
                return

            if self.sqlconn == None:
                raise util.Abort(
                    "invalid repo change - only hg push and pull" + " are allowed"
                )

            if not self.pendingrevs and not "bookmark_moved" in tr.hookargs:
                return

            reponame = self.sqlreponame
            cursor = self.sqlcursor

            if self.pendingrevs:
                self._validatependingrevs()

            try:
                self._addrevstosql(self.pendingrevs)

                # Compute new heads, and delete old heads
                newheads = set(hex(n) for n in self.heads())
                oldheads = []
                cursor.execute(
                    "SELECT value FROM revision_references "
                    "WHERE repo = %s AND namespace='heads'",
                    (reponame,),
                )
                for head in cursor:
                    head = head[0]
                    if head in newheads:
                        newheads.discard(head)
                    else:
                        oldheads.append(head)

                if oldheads:
                    headargs = ",".join(["%s"] * len(oldheads))
                    cursor.execute(
                        "DELETE revision_references FROM revision_references "
                        + "FORCE INDEX (bookmarkindex) "
                        + "WHERE namespace = 'heads' "
                        + "AND repo = %s AND value IN ("
                        + headargs
                        + ")",
                        (reponame,) + tuple(oldheads),
                    )

                # Compute new bookmarks, and delete old bookmarks
                newbookmarks = dict((k, hex(v)) for k, v in self._bookmarks.iteritems())
                oldbookmarks = []
                cursor.execute(
                    "SELECT name, value FROM revision_references "
                    "WHERE namespace = 'bookmarks' AND repo = %s",
                    (reponame,),
                )
                for k, v in cursor:
                    if newbookmarks.get(k) == v:
                        del newbookmarks[k]
                    else:
                        oldbookmarks.append(k)

                if oldbookmarks:
                    bookargs = ",".join(["%s"] * len(oldbookmarks))
                    cursor.execute(
                        "DELETE revision_references FROM revision_references "
                        + "FORCE INDEX (bookmarkindex) "
                        + "WHERE namespace = 'bookmarks' AND repo = %s "
                        + "AND name IN ("
                        + bookargs
                        + ")",
                        (repo.sqlreponame,) + tuple(oldbookmarks),
                    )

                tmpl = []
                values = []
                for head in newheads:
                    tmpl.append("(%s, 'heads', NULL, %s)")
                    values.append(reponame)
                    values.append(head)

                for k, v in newbookmarks.iteritems():
                    tmpl.append("(%s, 'bookmarks', %s, %s)")
                    values.append(repo.sqlreponame)
                    values.append(k)
                    values.append(v)

                if tmpl:
                    cursor.execute(
                        "INSERT INTO revision_references(repo, namespace, name, value) "
                        + "VALUES %s" % ",".join(tmpl),
                        tuple(values),
                    )

                # revision_references has multiple keys (primary key, and a unique index), so
                # mysql gives a warning when using ON DUPLICATE KEY since it would only update one
                # row despite multiple key duplicates. This doesn't matter for us, since we know
                # there is only one row that will share the same key. So suppress the warning.
                cursor.execute(
                    """INSERT INTO revision_references(repo, namespace, name, value)
                               VALUES(%s, 'tip', 'tip', %s) ON DUPLICATE KEY UPDATE value=%s""",
                    (reponame, len(self) - 1, len(self) - 1),
                )

                # Just to be super sure, check the write lock before doing the final commit
                if not self.hassqllock(writelock):
                    raise Exception(
                        "attempting to write to sql without holding %s (precommit)"
                        % writelock
                    )

                self.sqlconn.commit()
            except:
                self.sqlconn.rollback()
                raise
            finally:
                del self.pendingrevs[:]

        def _addrevstosql(self, revisions, ignoreduplicates=False):
            """Inserts the given revisions into the `revisions` table. If
            `ignoreduplicates` is True, the insert for that row is a no-op
            to allow ignoring existing rows during a bulk update.
            """

            def insert(args, values):
                query = (
                    "INSERT INTO revisions(repo, path, "
                    "chunk, chunkcount, linkrev, rev, node, entry, "
                    "data0, data1, createdtime) VALUES %s"
                )
                if ignoreduplicates:
                    # Do nothing
                    query += " ON DUPLICATE KEY UPDATE repo = %%s"
                    args = list(args)
                    values = list(values)
                    values.append(values[0])

                argstring = ",".join(args)
                cursor.execute(query % argstring, values)

            reponame = self.sqlreponame
            cursor = self.sqlcursor

            maxcommitsize = self.maxcommitsize
            maxinsertsize = self.maxinsertsize
            maxrowsize = self.maxrowsize
            commitsize = 0
            insertsize = 0

            args = []
            values = []

            now = time.strftime("%Y-%m-%d %H:%M:%S")
            for revision in revisions:
                path, linkrev, rev, node, entry, data0, data1 = revision

                start = 0
                chunk = 0
                datalen = len(data1)
                chunkcount = datalen / maxrowsize
                if datalen % maxrowsize != 0 or datalen == 0:
                    chunkcount += 1

                # We keep row size down by breaking large revisions down into
                # smaller chunks.
                while chunk == 0 or start < len(data1):
                    end = min(len(data1), start + maxrowsize)
                    datachunk = data1[start:end]
                    args.append("(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)")
                    values.extend(
                        (
                            reponame,
                            path,
                            chunk,
                            chunkcount,
                            linkrev,
                            rev,
                            node,
                            entry,
                            data0,
                            datachunk,
                            now,
                        )
                    )

                    size = len(datachunk)
                    commitsize += size
                    insertsize += size

                    chunk += 1
                    start = end

                    # Minimize roundtrips by doing bulk inserts
                    if insertsize > maxinsertsize:
                        insert(args, values)
                        del args[:]
                        del values[:]
                        insertsize = 0

                    # MySQL transactions can only reach a certain size, so we commit
                    # every so often.  As long as we don't update the tip pushkey,
                    # this is ok.
                    if commitsize > maxcommitsize:
                        self.sqlconn.commit()
                        commitsize = 0

            if args:
                insert(args, values)

            # commit at the end just to make sure we're clean
            self.sqlconn.commit()

        def _validatependingrevs(self):
            """Validates that the current pending revisions will be valid when
            written to the database.
            """
            reponame = self.sqlreponame
            cursor = self.sqlcursor

            # Ensure we hold the write lock
            if not self.hassqllock(writelock):
                raise Exception(
                    "attempting to write to sql without holding %s (prevalidate)"
                    % writelock
                )

            # Validate that we are appending to the correct linkrev
            cursor.execute(
                """SELECT value FROM revision_references WHERE repo = %s AND
                namespace = 'tip'""",
                (reponame,),
            )
            tipresults = cursor.fetchall()
            if len(tipresults) == 0:
                maxlinkrev = -1
            elif len(tipresults) == 1:
                maxlinkrev = int(tipresults[0][0])
            else:
                raise CorruptionException(
                    ("multiple tips for %s in " + " the database") % reponame
                )

            if (
                not util.safehasattr(self, "sqlreplaytransaction")
                or not self.sqlreplaytransaction
            ):
                minlinkrev = min(self.pendingrevs, key=lambda x: x[1])[1]
                if maxlinkrev == None or maxlinkrev != minlinkrev - 1:
                    raise CorruptionException(
                        "attempting to write non-sequential "
                        + "linkrev %s, expected %s" % (minlinkrev, maxlinkrev + 1)
                    )

            # Clean up excess revisions left from interrupted commits.
            # Since MySQL can only hold so much data in a transaction, we allow
            # committing across multiple db transactions. That means if
            # the commit is interrupted, the next transaction needs to clean
            # up bad revisions.
            cursor.execute(
                """DELETE FROM revisions WHERE repo = %s AND
                linkrev > %s""",
                (reponame, maxlinkrev),
            )

            # Validate that all rev dependencies (base, p1, p2) have the same
            # node in the database
            pending = set(
                [(path, rev) for path, _, rev, _, _, _, _ in self.pendingrevs]
            )
            expectedrevs = set()
            for revision in self.pendingrevs:
                path, linkrev, rev, node, entry, data0, data1 = revision
                e = revlog.indexformatng.unpack(entry)
                _, _, _, base, _, p1r, p2r, _ = e

                if p1r != nullrev and not (path, p1r) in pending:
                    expectedrevs.add((path, p1r))
                if p2r != nullrev and not (path, p2r) in pending:
                    expectedrevs.add((path, p2r))
                if base != nullrev and base != rev and not (path, base) in pending:
                    expectedrevs.add((path, base))

            if not expectedrevs:
                return

            missingrevs = []
            expectedlist = list(expectedrevs)
            expectedcount = len(expectedrevs)
            batchsize = self.ui.configint("hgsql", "verifybatchsize")
            i = 0
            while i < expectedcount:
                checkrevs = set(expectedlist[i : i + batchsize])
                i += batchsize

                whereclauses = []
                args = []
                args.append(reponame)
                for path, rev in checkrevs:
                    whereclauses.append("(path, rev, chunk) = (%s, %s, 0)")
                    args.append(path)
                    args.append(rev)

                whereclause = " OR ".join(whereclauses)
                cursor.execute(
                    """SELECT path, rev, node FROM revisions WHERE
                    repo = %s AND ("""
                    + whereclause
                    + ")",
                    args,
                )

                for path, rev, node in cursor:
                    rev = int(rev)
                    checkrevs.remove((path, rev))
                    rl = None
                    if path == "00changelog.i":
                        rl = self.changelog
                    elif path == "00manifest.i":
                        rl = self.manifestlog._revlog
                    else:
                        rl = revlog.revlog(self.svfs, path)
                    localnode = hex(rl.node(rev))
                    if localnode != node:
                        raise CorruptionException(
                            ("expected node %s at rev %d of " "%s but found %s")
                            % (node, rev, path, localnode)
                        )

                if len(checkrevs) > 0:
                    missingrevs.extend(checkrevs)

            if missingrevs:
                raise CorruptionException(
                    (
                        "unable to verify %d dependent "
                        + "revisions before adding a commit"
                    )
                    % (len(missingrevs))
                )

        def _afterlock(self, callback):
            if self.hassqllock(writelock, checkserver=False):
                self.sqlpostrelease.append(callback)
            else:
                return super(sqllocalrepo, self)._afterlock(callback)

    ui = repo.ui

    sqlargs = {}
    sqlargs["host"] = ui.config("hgsql", "host")
    sqlargs["database"] = ui.config("hgsql", "database")
    sqlargs["user"] = ui.config("hgsql", "user")
    sqlargs["port"] = ui.configint("hgsql", "port")
    password = ui.config("hgsql", "password")
    if password:
        sqlargs["password"] = password

    repo.sqlargs = sqlargs

    repo.sqlreponame = ui.config("hgsql", "reponame")
    if not repo.sqlreponame:
        raise Exception("missing hgsql.reponame")
    repo.maxcommitsize = ui.configbytes("hgsql", "maxcommitsize")
    repo.maxinsertsize = ui.configbytes("hgsql", "maxinsertsize")
    repo.maxrowsize = ui.configbytes("hgsql", "maxrowsize")
    repo.sqlconn = None
    repo.sqlcursor = None
    repo.disablesync = False
    repo.pendingrevs = []
    repo.heldlocks = set()
    repo.sqlpostrelease = []
    repo._issyncing = False

    repo.__class__ = sqllocalrepo


class bufferedopener(object):
    """Opener implementation that buffers all writes in memory until
    flush or close is called.
    """

    def __init__(self, opener, path, mode):
        self.opener = opener
        self.path = path
        self.mode = mode
        self.buffer = []
        self.closed = False

    def write(self, value):
        if self.closed:
            raise Exception("attempted to write to a closed bufferedopener")
        self.buffer.append(value)

    def flush(self):
        buffer = self.buffer
        self.buffer = []

        if buffer:
            fp = self.opener(self.path, self.mode)
            fp.write("".join(buffer))
            fp.close()

    def close(self):
        self.flush()
        self.closed = True


def addentries(repo, queue, transaction, ignoreexisting=False):
    """Reads new rev entries from a queue and writes them to a buffered
    revlog. At the end it flushes all the new data to disk.
    """
    opener = repo.svfs

    revlogs = {}

    def writeentry(revdata):
        # Instantiates pseudo-revlogs for us to write data directly to
        path, chunk, chunkcount, link, entry, data0, data1 = revdata
        revlog = revlogs.get(path)
        if not revlog:
            revlog = EntryRevlog(opener, path)
            revlogs[path] = revlog

        # Replace the existing openers with buffered ones so we can
        # perform the flush to disk all at once at the end.
        if not hasattr(revlog, "ifh") or revlog.ifh.closed:
            dfh = None
            if not revlog._inline:
                dfh = bufferedopener(opener, revlog.datafile, "a+")
            ifh = bufferedopener(opener, revlog.indexfile, "a+")
            revlog.ifh = ifh
            revlog.dfh = dfh

        revlog.addentry(
            transaction,
            revlog.ifh,
            revlog.dfh,
            entry,
            data0,
            data1,
            ignoreexisting=ignoreexisting,
        )

    leftover = None
    exit = False

    # Read one linkrev at a time from the queue
    while not exit:
        currentlinkrev = -1

        revisions = []
        if leftover:
            revisions.append(leftover)
            leftover = None

        # Read everything from the current linkrev
        while True:
            revdata = queue.get()
            if not revdata:
                exit = True
                break

            # The background thread had an exception, rethrow from the
            # foreground thread.
            if isinstance(revdata, Exception):
                raise revdata

            linkrev = revdata[3]
            if currentlinkrev == -1:
                currentlinkrev = linkrev
            if linkrev == currentlinkrev:
                revisions.append(revdata)
            elif linkrev < currentlinkrev:
                raise CorruptionException("SQL data is not in linkrev order")
            else:
                leftover = revdata
                currentlinkrev = linkrev
                break

        if not revisions:
            continue

        for revdata in revisions:
            writeentry(revdata)

    # Flush filelogs, then manifest, then changelog
    changelog = revlogs.pop("00changelog.i", None)
    manifest = revlogs.pop("00manifest.i", None)

    def flushrevlog(revlog):
        if not revlog.ifh.closed:
            revlog.ifh.flush()
        if revlog.dfh and not revlog.dfh.closed:
            revlog.dfh.flush()

    for filelog in revlogs.itervalues():
        flushrevlog(filelog)

    if manifest:
        flushrevlog(manifest)
    if changelog:
        flushrevlog(changelog)


class EntryRevlog(revlog.revlog):
    """Pseudo-revlog implementation that allows applying data directly to
    the end of the revlog.
    """

    def __init__(self, opener, path):
        super(EntryRevlog, self).__init__(opener, path)

        # This is a copy of the changelog init implementation.
        # It hard codes no generaldelta.
        if path == "00changelog.i" and self._initempty:
            self.version &= ~revlog.FLAG_GENERALDELTA
            self._generaldelta = False

    def addentry(
        self, transaction, ifh, dfh, entry, data0, data1, ignoreexisting=False
    ):
        curr = len(self)
        offset = self.end(curr - 1)

        e = revlog.indexformatng.unpack(entry)
        offsettype, datalen, textlen, base, link, p1r, p2r, node = e

        # The first rev has special metadata encoded in it that should be
        # stripped before being added to the index.
        if curr == 0:
            elist = list(e)
            type = revlog.gettype(offsettype)
            offsettype = revlog.offset_type(0, type)
            elist[0] = offsettype
            e = tuple(elist)

        if ignoreexisting and node in self.nodemap:
            return

        # Verify that the rev's parents and base appear earlier in the revlog
        if p1r >= curr or p2r >= curr:
            raise CorruptionException(
                "parent revision is not in revlog: %s" % self.indexfile
            )
        if base > curr:
            raise CorruptionException(
                "base revision is not in revlog: %s" % self.indexfile
            )

        expectedoffset = revlog.getoffset(offsettype)
        if expectedoffset != 0 and expectedoffset != offset:
            raise CorruptionException(
                "revision offset doesn't match prior length "
                + "(%s offset vs %s length): %s"
                % (expectedoffset, offset, self.indexfile)
            )

        if node not in self.nodemap:
            self.index.insert(-1, e)
            self.nodemap[node] = len(self) - 1

        if not self._inline:
            transaction.add(self.datafile, offset)
            transaction.add(self.indexfile, curr * len(entry))
            if data0:
                dfh.write(data0)
            dfh.write(data1)
            ifh.write(entry)
        else:
            offset += curr * self._io.size
            transaction.add(self.indexfile, offset, curr)
            ifh.write(entry)
            ifh.write(data0)
            ifh.write(data1)
            self.checkinlinesize(transaction, ifh)


def addgroup(orig, self, deltas, linkmapper, transaction, addrevisioncb=None):
    """Copy paste of revlog.addgroup, but we ensure that the revisions are
    added in linkrev order.
    """
    if not util.safehasattr(transaction, "repo"):
        return orig(self, deltas, linkmapper, transaction, addrevisioncb=addrevisioncb)

    # track the base of the current delta log
    content = []
    node = None

    r = len(self)
    end = 0
    if r:
        end = self.end(r - 1)
    ifh = self.opener(self.indexfile, "a+")
    isize = r * self._io.size
    if self._inline:
        transaction.add(self.indexfile, end + isize, r)
        dfh = None
    else:
        transaction.add(self.indexfile, isize, r)
        transaction.add(self.datafile, end)
        dfh = self.opener(self.datafile, "a+")

    try:
        # loop through our set of deltas
        chunkdatas = []
        chunkmap = {}

        lastlinkrev = -1
        reorder = False

        # Read all of the data from the stream
        for data in deltas:
            node, p1, p2, linknode, deltabase, delta, flags = data

            link = linkmapper(linknode)
            if link < lastlinkrev:
                reorder = True
            lastlinkrev = link
            chunkdata = {
                "node": node,
                "p1": p1,
                "p2": p2,
                "cs": linknode,
                "deltabase": deltabase,
                "delta": delta,
                "flags": flags,
            }
            chunkdatas.append((link, chunkdata))
            chunkmap[node] = chunkdata

        # If we noticed a incoming rev was not in linkrev order
        # we reorder all the revs appropriately.
        if reorder:
            chunkdatas = sorted(chunkdatas)

            fulltexts = {}

            def getfulltext(node):
                if node in fulltexts:
                    return fulltexts[node]
                if node in self.nodemap:
                    return self.revision(node, raw=True)

                chunkdata = chunkmap[node]
                deltabase = chunkdata["deltabase"]
                delta = chunkdata["delta"]

                deltachain = []
                currentbase = deltabase
                while True:
                    if currentbase in fulltexts:
                        deltachain.append(fulltexts[currentbase])
                        break
                    elif currentbase in self.nodemap:
                        deltachain.append(self.revision(currentbase, raw=True))
                        break
                    elif currentbase == nullid:
                        break
                    else:
                        deltachunk = chunkmap[currentbase]
                        currentbase = deltachunk["deltabase"]
                        deltachain.append(deltachunk["delta"])

                prevtext = deltachain.pop()
                while deltachain:
                    prevtext = mdiff.patch(prevtext, deltachain.pop())

                fulltext = mdiff.patch(prevtext, delta)
                fulltexts[node] = fulltext
                return fulltext

            visited = set()
            prevnode = self.node(len(self) - 1)
            for link, chunkdata in chunkdatas:
                node = chunkdata["node"]
                deltabase = chunkdata["deltabase"]
                if not deltabase in self.nodemap and not deltabase in visited:
                    fulltext = getfulltext(node)
                    ptext = getfulltext(prevnode)
                    delta = mdiff.textdiff(ptext, fulltext)

                    chunkdata["delta"] = delta
                    chunkdata["deltabase"] = prevnode

                prevnode = node
                visited.add(node)

        # Apply the reordered revs to the revlog
        for link, chunkdata in chunkdatas:
            node = chunkdata["node"]
            p1 = chunkdata["p1"]
            p2 = chunkdata["p2"]
            cs = chunkdata["cs"]
            deltabase = chunkdata["deltabase"]
            delta = chunkdata["delta"]
            flags = chunkdata["flags"] or revlog.REVIDX_DEFAULT_FLAGS

            content.append(node)

            link = linkmapper(cs)
            if node in self.nodemap:
                # this can happen if two branches make the same change
                continue

            for p in (p1, p2):
                if p not in self.nodemap:
                    raise LookupError(p, self.indexfile, _("unknown parent"))

            if deltabase not in self.nodemap:
                raise LookupError(deltabase, self.indexfile, _("unknown delta base"))

            baserev = self.rev(deltabase)
            chain = self._addrevision(
                node, None, transaction, link, p1, p2, flags, (baserev, delta), ifh, dfh
            )

            if addrevisioncb:
                # Data for added revision can't be read unless flushed
                # because _loadchunk always opensa new file handle and
                # there is no guarantee data was actually written yet.
                if dfh:
                    dfh.flush()
                ifh.flush()
                addrevisioncb(self, chain)

            if not dfh and not self._inline:
                # addrevision switched from inline to conventional
                # reopen the index
                ifh.close()
                dfh = self.opener(self.datafile, "a+")
                ifh = self.opener(self.indexfile, "a+")
    finally:
        if dfh:
            dfh.close()
        ifh.close()

    return content


def bookmarkcommand(orig, ui, repo, *names, **opts):
    if not issqlrepo(repo):
        return orig(ui, repo, *names, **opts)

    write = opts.get("delete") or opts.get("rename") or opts.get("inactive") or names

    def _bookmarkcommand():
        return orig(ui, repo, *names, **opts)

    if write:
        return executewithsql(repo, _bookmarkcommand, True)
    else:
        return _bookmarkcommand()


def pushkey(orig, repo, proto, namespace, key, old, new):
    if issqlrepo(repo):

        def commitpushkey():
            return orig(repo, proto, namespace, key, old, new)

        return executewithsql(repo, commitpushkey, namespace == "bookmarks")
    else:
        return orig(repo, proto, namespace, key, old, new)


# recover must be a norepo command because loading the repo fails
@command(
    "^sqlrecover",
    [
        ("f", "force", "", _("strips as far back as necessary"), ""),
        ("", "no-backup", None, _("does not produce backup bundles for strips")),
    ],
    _("hg sqlrecover"),
    norepo=True,
)
def sqlrecover(ui, *args, **opts):
    """
    Strips commits from the local repo until it is back in sync with the SQL
    server.
    """

    global initialsync
    initialsync = INITIAL_SYNC_DISABLE
    repo = hg.repository(ui, ui.environ["PWD"])
    repo.disablesync = True

    if repo.recover():
        ui.status("recovered from incomplete transaction")

    def iscorrupt():
        repo.sqlconnect()
        try:
            repo.syncdb()
        except:
            return True
        finally:
            repo.sqlclose()

        return False

    reposize = len(repo)

    stripsize = 10
    while iscorrupt() and len(repo) > 0:
        if not opts.get("force") and reposize > len(repo) + 10000:
            ui.warn(
                "unable to fix repo after stripping 10000 commits "
                + "(use -f to strip more)"
            )

        striprev = max(0, len(repo) - stripsize)
        nodelist = [repo[striprev].node()]
        ui.status("stripping back to %s commits" % (striprev))

        backup = not opts.get("no_backup")
        repair.strip(ui, repo, nodelist, backup=backup, topic="sqlrecover")

        stripsize = min(stripsize * 5, 10000)

    if len(repo) == 0:
        ui.warn(_("unable to fix repo corruption\n"))
    elif len(repo) == reposize:
        ui.status(_("local repo was not corrupt - no action taken\n"))
    else:
        ui.status(_("local repo now matches SQL\n"))


@command(
    "sqltreestrip",
    [
        (
            "",
            "local-only",
            None,
            _("only strips the commits locally, and not from sql"),
        ),
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run sqltreestrip if you " "know exactly what you're doing"),
        ),
    ],
    _("hg sqltreestrip REV"),
)
def sqltreestrip(ui, repo, rev, *args, **opts):
    """Strips trees from local and sql history
    """
    try:
        treemfmod = extensions.find("treemanifest")
    except KeyError:
        ui.warn(_("treemanifest is not enabled for this repository\n"))
        return 1

    if not repo.ui.configbool("treemanifest", "server"):
        ui.warn(_("this repository is not configured to be a treemanifest " "server\n"))
        return 1

    if not opts.get("i_know_what_i_am_doing"):
        raise util.Abort(
            "You must pass --i-know-what-i-am-doing to run this "
            + "command. If you have multiple servers using the database, this "
            + "command will break your servers until you run it on each one. "
            + "Only the Mercurial server admins should ever run this."
        )

    rev = int(rev)

    ui.warn(
        _(
            "*** YOU ARE ABOUT TO DELETE TREE HISTORY INCLUDING AND AFTER %s "
            "(MANDATORY 5 SECOND WAIT) ***\n"
        )
        % rev
    )
    import time

    time.sleep(5)

    if not opts.get("local_only"):
        # strip from sql
        reponame = repo.sqlreponame
        repo.sqlconnect()
        repo.sqllock(writelock)
        try:
            cursor = repo.sqlcursor
            ui.status(_("mysql: deleting trees with linkrevs >= %s\n") % rev)
            cursor.execute(
                """DELETE FROM revisions
                              WHERE repo = %s AND linkrev >= %s AND
                                    (path LIKE 'meta/%%' OR
                                     path = '00manifesttree.i')""",
                (reponame, rev),
            )
            repo.sqlconn.commit()
        finally:
            repo.sqlunlock(writelock)
            repo.sqlclose()

    # strip from local
    ui.status(_("local: deleting trees with linkrevs >= %s\n") % rev)
    with repo.wlock(), repo.lock(), repo.transaction("treestrip") as tr:
        repo.disablesync = True

        # Duplicating some logic from repair.py
        offset = len(tr.entries)
        tr.startgroup()
        files = treemfmod.collectfiles(None, repo, rev)
        treemfmod.striptrees(None, repo, tr, rev, files)
        tr.endgroup()

        for i in xrange(offset, len(tr.entries)):
            file, troffset, ignore = tr.entries[i]
            with repo.svfs(file, "a", checkambig=True) as fp:
                util.truncate(fp, troffset)
            if troffset == 0:
                repo.store.markremoved(file)


def _parsecompressedrevision(data):
    """Takes a compressed revision and parses it into the data0 (compression
    indicator) and data1 (payload). Ideally we'd refactor revlog.decompress to
    have this logic be separate, but there are comments in the code about perf
    implications of the hotpath."""
    t = data[0:1]

    if t == "u":
        return "u", data[1:]
    else:
        return "", data


def _discoverrevisions(repo, startrev):
    # Tuple of revlog name and rev number for revisions introduced by commits
    # greater than or equal to startrev (path, rlrev)
    revisions = []

    mfrevlog = repo.manifestlog._revlog
    for rev in repo.revs("%s:", startrev):
        # Changelog
        revisions.append(("00changelog.i", rev))

        # Manifestlog
        mfrev = mfrevlog.rev(repo[rev].manifestnode())
        revisions.append(("00manifest.i", mfrev))

    files = repair._collectfiles(repo, startrev)

    # Trees
    if repo.ui.configbool("treemanifest", "server"):
        rootmflog = repo.manifestlog.treemanifestlog._revlog
        striprev, brokenrevs = rootmflog.getstrippoint(startrev)
        for mfrev in range(striprev, len(rootmflog)):
            revisions.append((rootmflog.indexfile, mfrev))

        for dir in util.dirs(files):
            submflog = rootmflog.dirlog(dir)
            striprev, brokenrevs = submflog.getstrippoint(startrev)
            for mfrev in range(striprev, len(submflog)):
                revisions.append((submflog.indexfile, mfrev))

    # Files
    for file in files:
        filelog = repo.file(file)
        striprev, brokenrevs = filelog.getstrippoint(startrev)
        for filerev in range(striprev, len(filelog)):
            revisions.append((filelog.indexfile, filerev))

    return revisions


@command(
    "sqlrefill",
    [
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run sqlrefill if you know exactly what you're doing"),
        ),
        (
            "",
            "skip-initial-sync",
            None,
            _(
                "skips the initial sync; useful when the "
                "local repo is correct and the database "
                "is incorrect"
            ),
        ),
    ],
    _("hg sqlrefill REV"),
    norepo=True,
)
def sqlrefill(ui, startrev, **opts):
    """Inserts the given revs into the database
    """
    if not opts.get("i_know_what_i_am_doing"):

        raise util.Abort(
            "You must pass --i-know-what-i-am-doing to run this "
            "command. If you have multiple servers using the database, you "
            "will need to run sqlreplay on the other servers to get this "
            "data onto them as well."
        )

    if not opts.get("skip_initial_sync"):
        global initialsync
        initialsync = INITIAL_SYNC_DISABLE
        repo = hg.repository(ui, ui.environ["PWD"])
        repo.disablesync = True

    startrev = int(startrev)

    repo = repo.unfiltered()
    repo.sqlconnect()
    repo.sqllock(writelock)
    try:
        totalrevs = len(repo.changelog)

        revlogs = {}
        pendingrevs = []
        # with progress.bar(ui, 'refilling', total=totalrevs - startrev) as prog:
        # prog.value += 1
        revlogrevs = _discoverrevisions(repo, startrev)

        for path, rlrev in revlogrevs:
            rl = revlogs.get(path)
            if rl is None:
                rl = revlog.revlog(repo.svfs, path)
                revlogs[path] = rl

            entry = rl.index[rlrev]
            _, _, _, baserev, linkrev, p1r, p2r, node = entry
            data = rl._getsegmentforrevs(rlrev, rlrev)[1]
            data0, data1 = _parsecompressedrevision(data)

            sqlentry = rl._io.packentry(entry, node, rl.version, rlrev)
            revdata = (path, linkrev, rlrev, node, sqlentry, data0, data1)
            pendingrevs.append(revdata)

        repo._addrevstosql(pendingrevs, ignoreduplicates=True)
        repo.sqlconn.commit()
    finally:
        repo.sqlunlock(writelock)
        repo.sqlclose()


@command(
    "^sqlstrip",
    [
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run sqlstrip if you know " + "exactly what you're doing"),
        ),
        (
            "",
            "no-backup-permanent-data-loss",
            None,
            _("does not produce backup bundles (for use with corrupt revlogs)"),
        ),
    ],
    _("hg sqlstrip [OPTIONS] REV"),
    norepo=True,
)
def sqlstrip(ui, rev, *args, **opts):
    """strips all revisions greater than or equal to the given revision from the sql database

    Deletes all revisions with linkrev >= the given revision from the local
    repo and from the sql database. This is permanent and cannot be undone.
    Once the revisions are deleted from the database, you will need to run
    this command on each master server before proceeding to write new revisions.
    """

    if not opts.get("i_know_what_i_am_doing"):
        raise util.Abort(
            "You must pass --i-know-what-i-am-doing to run this "
            + "command. If you have multiple servers using the database, this "
            + "command will break your servers until you run it on each one. "
            + "Only the Mercurial server admins should ever run this."
        )

    ui.warn("*** YOU ARE ABOUT TO DELETE HISTORY (MANDATORY 5 SECOND WAIT) ***\n")
    import time

    time.sleep(5)

    backup = not opts.get("no_backup_permanent_data_loss")
    if not backup:
        ui.warn("*** *** *** *** *** *** *** *** * *** *** *** *** *** *** *** ***\n")
        ui.warn("*** THERE ARE NO BACKUPS!       *  (MANDATORY 10 SECOND WAIT) ***\n")
        ui.warn("*** *** *** *** *** *** *** *** * *** *** *** *** *** *** *** ***\n")
        time.sleep(10)

    global initialsync
    initialsync = INITIAL_SYNC_DISABLE
    repo = hg.repository(ui, ui.environ["PWD"])
    repo.disablesync = True

    try:
        rev = int(rev)
    except ValueError:
        raise util.Abort("specified rev must be an integer: '%s'" % rev)

    wlock = lock = None
    wlock = repo.wlock()
    lock = repo.lock()
    try:
        repo.sqlconnect()
        try:
            repo.sqllock(writelock)

            if rev not in repo:
                raise util.Abort("revision %s is not in the repo" % rev)

            reponame = repo.sqlreponame
            cursor = repo.sqlcursor
            changelog = repo.changelog

            revs = repo.revs("%s:" % rev)
            # strip locally
            ui.status("stripping locally\n")
            repair.strip(
                ui,
                repo,
                [changelog.node(r) for r in revs],
                backup=backup,
                topic="sqlstrip",
            )

            ui.status("stripping from the database\n")
            ui.status("deleting old references\n")
            cursor.execute(
                """DELETE FROM revision_references WHERE repo = %s""", (reponame,)
            )

            ui.status("adding new head references\n")
            for head in repo.heads():
                cursor.execute(
                    """INSERT INTO revision_references(repo, namespace, value)
                               VALUES(%s, 'heads', %s)""",
                    (reponame, hex(head)),
                )

            ui.status("adding new tip reference\n")
            cursor.execute(
                """INSERT INTO revision_references(repo, namespace, name, value)
                           VALUES(%s, 'tip', 'tip', %s)""",
                (reponame, len(repo) - 1),
            )

            ui.status("adding new bookmark references\n")
            for k, v in repo._bookmarks.iteritems():
                cursor.execute(
                    """INSERT INTO revision_references(repo, namespace, name, value)
                               VALUES(%s, 'bookmarks', %s, %s)""",
                    (reponame, k, hex(v)),
                )

            ui.status("deleting revision data\n")
            cursor.execute(
                """DELETE FROM revisions WHERE repo = %s and linkrev > %s""",
                (reponame, rev),
            )

            repo.sqlconn.commit()
        finally:
            repo.sqlunlock(writelock)
            repo.sqlclose()
    finally:
        if lock:
            lock.release()
        if wlock:
            wlock.release()


class CustomConverter(mysql.connector.conversion.MySQLConverter):
    """Ensure that all values being returned are returned as python string
    (versus the default byte arrays)."""

    def _STRING_to_python(self, value, dsc=None):
        return str(value)

    def _VAR_STRING_to_python(self, value, dsc=None):
        return str(value)

    def _BLOB_to_python(self, value, dsc=None):
        return str(value)


@command(
    "^sqlreplay",
    [
        ("", "start", "", _("the rev to start with"), ""),
        ("", "end", "", _("the rev to end with"), ""),
    ],
    _("hg sqlreplay"),
)
def sqlreplay(ui, repo, *args, **opts):
    """goes through the entire sql history and performs missing revlog writes

    This is useful for adding entirely new revlogs to history, like when
    converting repos to a new format. The replays perform the same validation
    before appending to a revlog, so you will never end up with incorrect
    revlogs from things being appended out of order.
    """
    maxrev = len(repo.changelog) - 1
    startrev = int(opts.get("start") or "0")
    endrev = int(opts.get("end") or str(maxrev))

    startrev = max(startrev, 0)
    endrev = min(endrev, maxrev)

    def _helper():
        _sqlreplay(repo, startrev, endrev)

    executewithsql(repo, _helper, False)


def _sqlreplay(repo, startrev, endrev):
    wlock = lock = None

    try:
        wlock = repo.wlock()
        lock = repo.lock()
        # Disable all pretxnclose hooks, since these revisions are
        # technically already commited.
        for name, value in repo.ui.configitems("hooks"):
            if name.startswith("pretxnclose"):
                repo.ui.setconfig("hooks", name, None)

        transaction = repo.transaction("sqlreplay")

        try:
            repo.sqlreplaytransaction = True
            queue = Queue.Queue()
            abort = threading.Event()

            t = threading.Thread(
                target=repo.fetchthread, args=(queue, abort, startrev, endrev)
            )
            t.setDaemon(True)
            try:
                t.start()
                addentries(repo, queue, transaction, ignoreexisting=True)
            finally:
                abort.set()

            transaction.close()
        finally:
            transaction.release()
            repo.sqlreplaytransaction = False
    finally:
        if lock:
            lock.release()
        if wlock:
            wlock.release()


@command(
    "^sqlverify",
    [("", "earliest-rev", "", _("the earliest rev to process"), "")],
    _("hg sqlverify"),
)
def sqlverify(ui, repo, *args, **opts):
    """verifies the current revlog indexes match the data in mysql

    Runs in reverse order, so it verifies the latest commits first.
    """
    maxrev = len(repo.changelog) - 1
    minrev = int(opts.get("earliest_rev") or "0")

    def _helper():
        stepsize = 1000
        firstrev = max(minrev, maxrev - stepsize)
        lastrev = maxrev

        insql = set()
        revlogcache = {}
        with progress.bar(ui, "verifying", total=maxrev - minrev) as prog:
            while True:
                insql.update(_sqlverify(repo, firstrev, lastrev, revlogcache))
                prog.value = maxrev - firstrev
                if firstrev == minrev:
                    break
                lastrev = firstrev - 1
                firstrev = max(minrev, firstrev - stepsize)

        # Check that on disk revlogs don't have extra information that isn't in
        # hgsql
        earliestmtime = time.time() - (3600 * 24 * 7)
        corrupted = False
        with progress.bar(ui, "verifying revlogs") as prog:
            for filepath, x, x in repo.store.walk():
                prog.value += 1
                if filepath[-2:] != ".i":
                    continue

                # If the revlog is recent, check it
                stat = repo.svfs.lstat(path=filepath)
                if stat.st_mtime <= earliestmtime:
                    continue

                rl = revlogcache.get(filepath)
                if rl is None:
                    if filepath == "00changelog.i":
                        rl = repo.unfiltered().changelog
                    elif filepath == "00manifest.i":
                        rl = repo.manifestlog._revlog
                    else:
                        rl = revlog.revlog(repo.svfs, filepath)
                for rev in xrange(len(rl) - 1, -1, -1):
                    node = rl.node(rev)
                    linkrev = rl.linkrev(rev)
                    if linkrev < minrev:
                        break
                    if (filepath, node) not in insql:
                        corrupted = True
                        msg = (
                            "corruption: '%s:%s' with linkrev %s "
                            "exists on local disk, but not in sql\n"
                        ) % (filepath, hex(node), linkrev)
                        repo.ui.status(msg)

        if corrupted:
            raise error.Abort("Verification failed")

        ui.status("Verification passed\n")

    executewithsql(repo, _helper, False)


def _sqlverify(repo, minrev, maxrev, revlogcache):
    queue = Queue.Queue()
    abort = threading.Event()
    t = threading.Thread(target=repo.fetchthread, args=(queue, abort, minrev, maxrev))
    t.setDaemon(True)

    insql = set()
    try:
        t.start()

        while True:
            revdata = queue.get()
            if not revdata:
                return insql

            # The background thread had an exception, rethrow from the
            # foreground thread.
            if isinstance(revdata, Exception):
                raise revdata

            # Validate revdata
            path = revdata[0]
            linkrev = revdata[3]
            packedentry = revdata[4]

            sqlentry = revlog.indexformatng.unpack(packedentry)

            rl = revlogcache.get(path)
            if rl is None:
                if path == "00changelog.i":
                    rl = repo.unfiltered().changelog
                elif path == "00manifest.i":
                    rl = repo.manifestlog._revlog
                else:
                    rl = revlog.revlog(repo.svfs, path)
                revlogcache[path] = rl

            node = sqlentry[7]
            rev = rl.rev(node)
            if rev == 0:
                # The first entry has special whole-revlog flags in place of the
                # offset in entry[0] that are not returned from revlog.index,
                # so strip that data.
                type = revlog.gettype(sqlentry[0])
                sqlentry = (revlog.offset_type(0, type),) + sqlentry[1:]

            insql.add((path, node))

            revlogentry = rl.index[rev]
            if revlogentry != sqlentry:
                raise CorruptionException(
                    ("'%s:%s' with linkrev %s, disk does " "not match mysql")
                    % (path, hex(node), str(linkrev))
                )
    finally:
        abort.set()
