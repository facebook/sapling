# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# no-check-code
"""sync @prog@ repos with MySQL

Config::

    [hgsql]
    # Seconds. If it's non-negative. Try pulling from the database in a loop
    # before entering the critical section. This is done by changing the SQL lock
    # timeout to syncinterval, which should be much smaller than locktimeout.
    # This could make the repo closer to the SQL state after entering the
    # critical section so pulling from database might take less time there. If
    # it's a negative value, do not sync before entering the critical section.
    # (default: -1)
    syncinterval = -1

    # Seconds. Do not attempt to sync for read-only commands. If last sync was
    # within the specified time.
    synclimit = 30

    # Enable faster "need sync or not" check. It could be 6x faster, and
    # removes some time in the critical section.
    # (default: true)
    fastsynccheck = true

    # Whether to do an initial "best-effort" pull from the database.
    # (default: true)
    initialsync = true

    # Timeout (in seconds) set at the socket connection.
    sockettimeout = 60
"""


from __future__ import absolute_import

import hashlib
import os
import sys
import threading
import time
import warnings
from types import ModuleType
from typing import Optional, Sized, Tuple

from edenscm import (
    bookmarks,
    bundle2,
    bundlerepo,
    changegroup,
    commands,
    error,
    exchange,
    extensions,
    hg,
    hgdemandimport as demandimport,
    localrepo,
    lock as lockmod,
    mdiff,
    progress,
    pycompat,
    registrar,
    repair,
    revlog,
    util,
    wireproto,
)
from edenscm.i18n import _, _x
from edenscm.node import bin, hex, nullid, nullrev
from edenscm.pycompat import decodeutf8, encodeutf8, queue, range


wrapcommand = extensions.wrapcommand
wrapfunction = extensions.wrapfunction

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "3.9.1"

configtable = {}
configitem = registrar.configitem(configtable)

configitem("hgsql", "database", default=None)
configitem("hgsql", "replicadatabase", default=None)
configitem("hgsql", "host", default=None)
configitem("hgsql", "reponame", default=None)
configitem("hgsql", "password", default="")
configitem("hgsql", "port", default=0)
configitem("hgsql", "user", default=None)

configitem("format", "usehgsql", default=True)
configitem("hgsql", "bypass", default=False)
configitem("hgsql", "enabled", default=False)
configitem("hgsql", "engine", default="innodb")
configitem("hgsql", "fastsynccheck", default=True)
configitem("hgsql", "initialsync", default=True)
configitem("hgsql", "locktimeout", default=60)
configitem("hgsql", "maxcommitsize", default=52428800)
configitem("hgsql", "maxinsertsize", default=1048576)
configitem("hgsql", "maxrowsize", default=1048576)
configitem("hgsql", "profileoutput", default="/tmp")
configitem("hgsql", "profiler", default=None)
configitem("hgsql", "rootpidnsonly", default=False)
configitem("hgsql", "sockettimeout", default=60)
configitem("hgsql", "sqltimeout", default=120)
configitem("hgsql", "syncinterval", default=-1)
configitem("hgsql", "synclimit", default=0)
configitem("hgsql", "verbose", default=False)
configitem("hgsql", "verifybatchsize", default=1000)
configitem("hgsql", "waittimeout", default=300)

# developer config: hgsql.debugminsqllockwaittime
# do not get the sql lock unless specified time has passed
configitem("hgsql", "debugminsqllockwaittime", default=0)

mysql: Optional[ModuleType] = None

writelock = "write_lock"

INITIAL_SYNC_NORMAL = "normal"
INITIAL_SYNC_DISABLE = "disabled"
INITIAL_SYNC_FORCE = "force"

initialsync: str = INITIAL_SYNC_NORMAL
initialsyncfromreplica = False

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


def isrootpidns() -> bool:
    """False if we're sure it's not a root pid namespace. True otherwise."""
    try:
        # Linux implementation detail - inode number is 0xeffffffc for root
        # namespaces.
        return os.stat("/proc/self/ns/pid").st_ino == 0xEFFFFFFC
    except Exception:
        # Cannot tell (not Linux, or no /proc mounted).
        return True


def ishgsqlbypassed(ui):
    # developer config: hgsql.rootpidnsonly
    if ui.configbool("hgsql", "rootpidnsonly") and not isrootpidns():
        return True
    return ui.configbool("hgsql", "bypass")


def issqlrepo(repo):
    return repo.ui.configbool("hgsql", "enabled")


def cansyncwithsql(repo):
    return issqlrepo(repo) and not isinstance(repo, bundlerepo.bundlerepository)


def uisetup(ui) -> None:
    # hgsql is incompatible with visibleheads - hgsql always wants all heads.
    ui.setconfig("visibility", "enabled", "false", "hgsql")
    # hgsql is incompatible with narrow-heads - hgsql always wants all heads.
    ui.setconfig("experimental", "narrow-heads", "false", "hgsql")
    # hgsql wants the legacy revlog access for changelog.
    ui.setconfig("experimental", "rust-commits-changelog", "false", "hgsql")
    # hgsql does not want any kind of filtering - everything is public.
    ui.setconfig("mutation", "enabled", "false", "hgsql")

    if ishgsqlbypassed(ui):
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
        hexnode = hex(e[7])
        data0 = data[0] or b""
        transaction.repo.pendingrevs.append(
            (self.indexfile, link, len(self) - 1, hexnode, entry, data0, data[1])
        )
        return orig(self, transaction, ifh, dfh, entry, data, link, offset)

    wrapfunction(revlog.revlog, "_writeentry", writeentry)

    # Reorder incoming revs to be in linkrev order
    wrapfunction(revlog.revlog, "addgroup", addgroup)

    def memcommitwrapper(loaded):
        if loaded:
            memcommitmod = extensions.find("memcommit")
            wrapfunction(memcommitmod, "_memcommit", _memcommit)

    extensions.afterloaded("memcommit", memcommitwrapper)


def _importmysqlconnector() -> None:
    # mysql.connector does not import nicely with the demandimporter, so
    # temporarily disable it.
    with demandimport.deactivated():
        global mysql
        mysql = __import__("mysql.connector")


def extsetup(ui) -> None:
    # Importing MySQL connector here allows other modules to import code from
    # `hgsql` without requiring the MySQL connector. There are cases where this
    # makes sense. For example, there is no need for MySQL connector on a
    # repository which does not have `hgsql` enabled.
    _importmysqlconnector()

    if ishgsqlbypassed(ui):
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
        commands.globalopts.append(
            (
                "",
                "syncfromreplica",
                False,
                _("do hgsql sync but prefer to sync from db replica"),
                _("TYPE"),
            )
        )

    global initialsync
    if not ui.configbool("hgsql", "initialsync"):
        initialsync = INITIAL_SYNC_DISABLE

    # Directly examining argv seems like a terrible idea, but it seems
    # necessary unless we refactor mercurial dispatch code. This is because
    # the first place we have access to parsed options is in the same function
    # (dispatch.dispatch) that created the repo and the repo creation initiates
    # the sync operation in which the lock is elided unless we set this.

    global initialsyncfromreplica
    if "--syncfromreplica" in sys.argv:
        ui.debug("syncing from replica\n")
        initialsyncfromreplica = True

    if "--forcesync" in sys.argv:
        ui.debug("forcesync enabled\n")
        initialsync = INITIAL_SYNC_FORCE


def reposetup(ui, repo) -> None:
    if ishgsqlbypassed(ui):
        return

    if issqlrepo(repo):
        wraprepo(repo)

        if initialsync != INITIAL_SYNC_DISABLE and cansyncwithsql(repo):
            # Use a noop to force a sync
            def noop():
                pass

            enforcepullfromdb = False
            syncfromreplica = False
            if initialsync == INITIAL_SYNC_FORCE:
                enforcepullfromdb = True

            if initialsyncfromreplica:
                syncfromreplica = True

            overrides = {}
            replicadbconfig = repo.ui.config("hgsql", "replicadatabase")
            if syncfromreplica:
                if replicadbconfig:
                    overrides[("hgsql", "database")] = replicadbconfig
                else:
                    ui.warn(
                        "--syncfromreplica is set, but hgsql.replicadatabase is not specified!\n"
                    )

            with ui.configoverride(overrides, "sqldbsync"):
                executewithsql(
                    repo,
                    noop,
                    enforcepullfromdb=enforcepullfromdb,
                    syncfromreplica=syncfromreplica,
                )


# Incoming commits are only allowed via push and pull.
# This changes repo.transaction to take a sql write lock. Note:
# repo.transaction outside unbundle is not changed. Question: is it better
# to change repo.transaction for everything?
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
            context = sqlcontext(repo, dbwritable=True, enforcepullfromdb=True)

            # Temporarily replace bundleoperation so we can hook into it's
            # locking mechanism.
            oldopclass = bundle2.bundleoperation

            repolocks = []

            class sqllockedoperation(bundle2.bundleoperation):
                def __init__(self, repo, transactiongetter, *args, **kwargs):
                    def sqllocktr():
                        # Get local repo locks first.
                        repolocks.append(repo.wlock(wait=True))
                        repolocks.append(repo.lock(wait=True))
                        if not context.active():
                            # Get global SQL transaction inside local lock.
                            # This could change the repo.
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
            for lock in reversed(repolocks):
                lockmod.release(lock)
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


def _memcommit(orig, *args, **kwargs):
    repo = args[0]
    if issqlrepo(repo):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)


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
    def __init__(
        self, repo, dbwritable=False, enforcepullfromdb=False, syncfromreplica=False
    ):
        if dbwritable and not enforcepullfromdb:
            raise error.ProgrammingError(
                "enforcepullfromdb must be True if dbwritable is True"
            )
        self.repo = repo
        self.dbwritable = dbwritable
        self.enforcepullfromdb = enforcepullfromdb
        self.syncfromreplica = syncfromreplica
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
        repo._sqlreadrows = 0
        repo._sqlwriterows = 0

        if not repo.sqlconn:
            repo.sqlconnect()
            self._connected = True

        if self.dbwritable and not writelock in repo.heldlocks:
            startwait = time.time()
            try:
                repo.sqlwritelock(trysync=True)
            except error.Abort:
                elapsed = time.time() - startwait
                repo.ui.log(
                    "sqllock",
                    "failed to get sql lock after %s seconds\n",
                    elapsed,
                    elapsed=elapsed * 1000,
                    valuetype="lockwait",
                    success="false",
                    repository=repo.root,
                )
                repo._hgsqlnote("failed to get lock after %.2f seconds" % elapsed)
                raise

            self._startprofile()
            self._locked = True
            elapsed = time.time() - startwait
            repo.ui.log(
                "sqllock",
                "waited for sql lock for %s seconds (read %s rows)\n",
                elapsed,
                repo._sqlreadrows,
                elapsed=elapsed * 1000,
                valuetype="lockwait",
                success="true",
                readrows=repo._sqlreadrows,
                repository=repo.root,
            )
            repo._hgsqlnote(
                "got lock after %.2f seconds (read %s rows)"
                % (elapsed, repo._sqlreadrows)
            )
            repo._sqlreadrows = 0
        self._startlocktime = time.time()

        if self._connected:
            repo.pullfromdb(
                enforcepullfromdb=self.enforcepullfromdb,
                syncfromreplica=self.syncfromreplica,
            )

    def __exit__(self, type, value, traceback):
        try:
            repo = self.repo
            if self._locked:
                elapsed = time.time() - self._startlocktime
                repo.sqlwriteunlock()
                self._stopprofile(elapsed)
                repo.ui.log(
                    "sqllock",
                    "held sql lock for %s seconds (read %s rows; write %s rows)\n",
                    elapsed,
                    repo._sqlreadrows,
                    repo._sqlwriterows,
                    elapsed=elapsed * 1000,
                    valuetype="lockheld",
                    repository=repo.root,
                    readrows=repo._sqlreadrows,
                    writerows=repo._sqlwriterows,
                )
                repo._hgsqlnote(
                    "held lock for %.2f seconds (read %s rows; write %s rows)"
                    % (elapsed, repo._sqlreadrows, repo._sqlwriterows)
                )

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
            from edenscm import lsprof

            self._profiler = lsprof.Profiler()
            self._profiler.enable(subcalls=True)
        elif profiler == "stat":
            from edenscm import statprof

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
            from edenscm import lsprof

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
            from edenscm import statprof

            statprof.stop()
            path = os.path.join(
                outputdir, "hgsql-profile-%s-%s-%s" % (pid, timestamp, rand)
            )
            with open(path, "a+") as f:
                statprof.display(f)
                f.write("Total Elapsed Time: %s\n" % elapsed)


def executewithsql(repo, action, sqllock: bool = False, *args, **kwargs):
    """Executes the given action while having a SQL connection open.
    If a locks are specified, those locks are held for the duration of the
    action.
    """
    # executewithsql can be executed in a nested scenario (ex: writing
    # bookmarks during a pull), so track whether this call performed
    # the connect.

    enforcepullfromdb = sqllock
    if "enforcepullfromdb" in kwargs:
        if not enforcepullfromdb:
            enforcepullfromdb = kwargs["enforcepullfromdb"]
        del kwargs["enforcepullfromdb"]

    syncfromreplica = False
    if "syncfromreplica" in kwargs:
        syncfromreplica = kwargs["syncfromreplica"]
        del kwargs["syncfromreplica"]

    # Take repo lock if sqllock is set.
    if sqllock:
        wlock = repo.wlock()
        lock = repo.lock()
    else:
        wlock = util.nullcontextmanager()
        lock = util.nullcontextmanager()

    with wlock, lock, sqlcontext(
        repo,
        dbwritable=sqllock,
        enforcepullfromdb=enforcepullfromdb,
        syncfromreplica=syncfromreplica,
    ):
        return action(*args, **kwargs)


def wraprepo(repo) -> None:
    class sqllocalrepo(repo.__class__):
        def sqlconnect(self):
            if self.sqlconn:
                return

            retry = 3
            while True:
                try:
                    try:
                        self.sqlconn = mysql.connector.connect(
                            force_ipv6=True, ssl_disabled=True, **self.sqlargs
                        )
                    except AttributeError:
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
            sqltimeout = self.ui.configint("hgsql", "sqltimeout") * 1000
            waittimeout = self.sqlconn.converter.escape("%s" % waittimeout)

            self.engine = self.ui.config("hgsql", "engine")
            self.locktimeout = self.ui.config("hgsql", "locktimeout")
            self.locktimeout = self.sqlconn.converter.escape("%s" % self.locktimeout)

            self.sqlcursor = self.sqlconn.cursor()
            self._sqlreadrows = 0
            self._sqlwriterows = 0

            # Patch sqlcursor so it updates the read write counters.
            def _fetchallupdatereadcount(orig):
                result = orig()
                self._sqlreadrows += self.sqlcursor.rowcount
                return result

            def _executeupdatewritecount(orig, sql, *args, **kwargs):
                result = orig(sql, *args, **kwargs)
                # naive ways to detect "writes"
                if sql.split(" ", 1)[0].upper() in {"DELETE", "UPDATE", "INSERT"}:
                    self._sqlwriterows += self.sqlcursor.rowcount
                return result

            wrapfunction(self.sqlcursor, "fetchall", _fetchallupdatereadcount)
            wrapfunction(self.sqlcursor, "execute", _executeupdatewritecount)

            self.sqlcursor.execute("SET wait_timeout=%s" % waittimeout)
            self.sqlcursor.execute("SET SESSION MAX_STATEMENT_TIME=%s" % sqltimeout)

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

        def _hgsqlnote(self, message):
            if self.ui.configbool("hgsql", "verbose"):
                self.ui.write_err("[hgsql] %s\n" % message)
            self.ui.debug("%s\n" % message)

        def _lockname(self, name):
            lockname = "%s_%s" % (name, self.sqlreponame)
            return self.sqlconn.converter.escape(lockname)

        def _sqllock(self, name, trysync):
            """If trysync is True, try to sync the repo outside the lock so it
            stays closer to the actual repo when the lock is acquired.
            """
            lockname = self._lockname(name)
            syncinterval = float(self.ui.config("hgsql", "syncinterval") or -1)
            if syncinterval < 0:
                trysync = False

            if trysync:
                minwaittime = self.ui.configint("hgsql", "debugminsqllockwaittime")
                # SELECT GET_LOCK(...) will block. Break the lock attempt into
                # smaller lock attempts
                starttime = time.time()
                locktimeout = float(self.locktimeout)
                while True:
                    elapsed = time.time() - starttime
                    if elapsed >= locktimeout:
                        raise util.Abort(
                            "timed out waiting for mysql repo lock (%s)" % lockname
                        )

                    # Sync outside the SQL lock hoping that the repo is closer
                    # to the SQL repo when we got the lock.
                    self.pullfromdb(enforcepullfromdb=True)
                    if elapsed < minwaittime:
                        # Pretend we wait and timed out, without actually
                        # getting the SQL lock. This is useful for testing.
                        time.sleep(syncinterval)
                    else:
                        # Try to acquire SQL lock, with a small timeout. So
                        # "forcesync" can get executed more frequently.
                        self.sqlcursor.execute(
                            "SELECT GET_LOCK('%s', %s)" % (lockname, syncinterval)
                        )
                        result = int(self.sqlcursor.fetchall()[0][0])
                        if result == 1:
                            break
            else:
                self.sqlcursor.execute(
                    "SELECT GET_LOCK('%s', %s)" % (lockname, self.locktimeout)
                )
                # cast to int to prevent passing bad sql data
                result = int(self.sqlcursor.fetchall()[0][0])
                if result != 1:
                    raise util.Abort(
                        "timed out waiting for mysql repo lock (%s)" % lockname
                    )
            self.heldlocks.add(name)

        def sqlwritelock(self, trysync=False):
            self._enforcelocallocktaken()
            self._sqllock(writelock, trysync)

        def _hassqllock(self, name, checkserver=True):
            if not name in self.heldlocks:
                return False

            if not checkserver:
                return True

            lockname = self._lockname(name)
            self.sqlcursor.execute("SELECT IS_USED_LOCK('%s')" % (lockname,))
            lockheldby = self.sqlcursor.fetchall()[0][0]
            if lockheldby is None:
                raise Exception("unable to check %s lock" % lockname)

            self.sqlcursor.execute("SELECT CONNECTION_ID()")
            myconnectid = self.sqlcursor.fetchall()[0][0]
            if myconnectid is None:
                raise Exception("unable to read connection id")

            return lockheldby == myconnectid

        def hassqlwritelock(self, checkserver=True):
            return self._hassqllock(writelock, checkserver)

        def _sqlunlock(self, name):
            lockname = self._lockname(name)
            self.sqlcursor.execute("SELECT RELEASE_LOCK('%s')" % (lockname,))
            self.sqlcursor.fetchall()
            self.heldlocks.discard(name)

            for callback in self.sqlpostrelease:
                callback()
            self.sqlpostrelease = []

        def sqlwriteunlock(self):
            self._enforcelocallocktaken()
            self._sqlunlock(writelock)

        def _enforcelocallocktaken(self):
            if self._issyncing:
                return
            if self._currentlock(self._lockref):
                return
            raise error.ProgrammingError("invalid lock order")

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

        def needsyncfast(self):
            """Returns True if the local repo might be out of sync.
            False otherwise.

            Faster than needsync. But do not return bookmarks or heads.
            """
            # Calculate local checksum in background.
            localsynchashes = []
            localthread = threading.Thread(
                target=lambda results, repo: results.append(repo._localsynchash()),
                args=(localsynchashes, self),
            )
            localthread.start()
            # Let MySQL do the same calculation on their side
            sqlsynchash = self._sqlsynchash()
            localthread.join()
            return sqlsynchash != localsynchashes[0]

        def _localsynchash(self):
            refs = dict(self._bookmarks)
            refs["tip"] = self["tip"].rev()
            sha = ""
            for k, v in sorted(pycompat.iteritems(refs)):
                if k != "tip":
                    v = hex(v)
                sha = hashlib.sha1(encodeutf8("%s%s%s" % (sha, k, v))).hexdigest()
            return sha

        def _sqlsynchash(self):
            sql = """
            SET @sha := '', @id = 0;
            SELECT sha FROM (
                SELECT
                    @id := @id + 1 as id,
                    @sha := sha1(concat(@sha, name, value)) as sha
                FROM revision_references
                WHERE repo = %s AND namespace IN ('bookmarks', 'tip') ORDER BY name
            ) AS t ORDER BY id DESC LIMIT 1;
            """

            sqlresults = [
                sqlresult.fetchall()
                for sqlresult in repo.sqlcursor.execute(
                    sql, (self.sqlreponame,), multi=True
                )
                if sqlresult.with_rows
            ]
            # is it a new repo with empty references?
            if sqlresults == [[]]:
                return hashlib.sha1(encodeutf8("%s%s" % ("tip", -1))).hexdigest()
            # sqlresults looks like [[('59237a7416a6a1764ea088f0bc1189ea58d5b592',)]]
            sqlsynchash = sqlresults[0][0][0]
            if len(sqlsynchash) != 40:
                raise RuntimeError("malicious SHA1 returned by MySQL: %r" % sqlsynchash)
            return decodeutf8(sqlsynchash)

        def needsync(self):
            """Returns True if the local repo is not in sync with the database.
            If it returns False, the heads and bookmarks match the database.

            Also return bookmarks and heads.
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
                if namespace == b"heads":
                    sqlheads.add(bin(value))
                elif namespace == b"bookmarks":
                    sqlbookmarks[decodeutf8(name)] = bin(value)
                elif namespace == b"tip":
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

            synclimit = self.ui.configint("hgsql", "synclimit")
            if synclimit > 0:
                lastsync = 0
                try:
                    lastsync = int(self.sharedvfs.tryread("lastsqlsync"))
                except Exception:
                    # This can happen if the file cannot be read or is not an int.
                    # Not fatal.
                    pass
                if time.time() - lastsync < synclimit:
                    # Hit limit. Skip sync.
                    self._hgsqlnote("skipping database sync due to rate limit")
                    return None
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
                # Someone else is already checking and updating the repo
                self._hgsqlnote(
                    "skipping database sync because another "
                    "process is already syncing"
                )
                return None

        def pullfromdb(self, enforcepullfromdb=False, syncfromreplica=False):
            """Attempts to sync the local repository with the latest bits in the
            database.

            If `enforcepullfromdb` is False, the sync is on a best effort basis,
            and the repo may not actually be up-to-date afterwards. If
            `enforcepullfromdb` is True, we guarantee that the repo is up-to-date when
            this function returns, otherwise an exception will be thrown."""
            try:
                self._issyncing = True
                if enforcepullfromdb:
                    return self._pullfromdb(enforcepullfromdb, syncfromreplica)
                else:
                    # For operations that do not require the absolute latest bits,
                    # only let one process update the repo at a time.
                    limiter = self.synclimiter()
                    if not limiter:

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
                        return self._pullfromdb(enforcepullfromdb, syncfromreplica)
                    finally:
                        limiter.release()
            finally:
                self._issyncing = False

        def _pullfromdb(self, enforcepullfromdb, syncfromreplica=False):
            # MySQL could take a snapshot of the database view.
            # Start a new transaction to get new changes.
            self.sqlconn.rollback()
            if self.ui.configbool("hgsql", "fastsynccheck"):
                if not self.needsyncfast():
                    self.ui.debug("syncing not needed\n")
                    return
            else:
                if not self.needsync()[0]:
                    self.ui.debug("syncing not needed\n")
                    return
            self.ui.debug("syncing with mysql\n")

            # Save a copy of the old manifest cache so we can put it back
            # afterwards.
            oldmancache = self.manifestlog._dirmancache

            wlock = util.nullcontextmanager()
            lock = util.nullcontextmanager()
            try:
                wlock = self.wlock(wait=enforcepullfromdb)
                lock = self.lock(wait=enforcepullfromdb)
            except error.LockHeld:
                if enforcepullfromdb:
                    raise
                # Oh well. Don't block this non-critical read-only operation.
                self._hgsqlnote("skipping sync for current operation")
                return

            # Disable all pretxnclose hooks, since these revisions are
            # technically already committed.
            overrides = {}
            for name, value in ui.configitems("hooks"):
                # The hg-ssh wrapper installs a hook to block all writes. We need to
                # circumvent this when we sync from the server.
                if name.startswith("pretxnclose") or name in {
                    "pretxnopen.hg-ssh",
                    "pretxnopen.hg-rsh",
                    "pretxnopen.readonlyrejectpush",
                }:
                    overrides[("hooks", name)] = None

            with ui.configoverride(overrides, "hgsql"), wlock, lock:
                outofsync, sqlheads, sqlbookmarks, fetchend = self.needsync()
                if not outofsync:
                    return
                # Local repository is ahead of replica - no need to sync
                if syncfromreplica and outofsync:
                    if len(self) - 1 > fetchend:
                        return

                self._hgsqlnote(
                    "getting %s commits from database"
                    % (fetchend - len(self.changelog) + 1)
                )
                transaction = self.transaction("pullfromdb")

                self.hook("presyncdb", throw=True)

                try:
                    # Inspect the changelog now that we have the lock
                    fetchstart = len(self.changelog)

                    q = queue.Queue()
                    abort = threading.Event()

                    t = threading.Thread(
                        target=self.fetchthread, args=(q, abort, fetchstart, fetchend)
                    )
                    t.setDaemon(True)
                    try:
                        t.start()
                        addentries(self, q, transaction)
                    finally:
                        abort.set()

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
                for dirname, lrucache in pycompat.iteritems(oldmancache):
                    if dirname == "":
                        for oldmfnode in lrucache:
                            oldmfctx = lrucache[oldmfnode]
                            if oldmfctx._data is not None:
                                mfl[oldmfnode]._data = oldmfctx._data

                if len(self) - 1 != fetchend:
                    raise CorruptionException(
                        "tip doesn't match after sync (self: %s, fetchend: %s)"
                        % (len(self) - 1, fetchend)
                    )

                heads = set(self.heads())
                heads.discard(nullid)
                if heads != sqlheads:
                    selfonly = map(hex, sorted(heads - sqlheads))
                    sqlonly = map(hex, sorted(sqlheads - heads))
                    raise CorruptionException(
                        "heads don't match after sync: (self: %r, sql: %r)"
                        % (selfonly, sqlonly)
                    )

                self.disablesync = True
                transaction = self.transaction("pullfromdb_bookmarks")
                try:
                    bm = self._bookmarks

                    self.sqlcursor.execute(
                        """SELECT name, value FROM revision_references
                        WHERE namespace = 'bookmarks' AND repo = %s""",
                        (self.sqlreponame,),
                    )
                    fetchedbookmarks = [
                        (decodeutf8(name), node)
                        for name, node in self.sqlcursor.fetchall()
                    ]

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

            # Since we just exited the lock, the changelog and bookmark
            # in-memory structures will need to be reloaded. If we loaded
            # changelog before bookmarks, we might accidentally load bookmarks
            # that don't exist in the loaded changelog. So let's force loading
            # bookmarks now.
            bm = self._bookmarks
            self.sharedvfs.write("lastsqlsync", encodeutf8(str(int(time.time()))))

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
                        revdata = (decodeutf8(revdata[0]),) + revdata[1:]

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
                    for chunks in pycompat.itervalues(groupedrevdata):
                        chunkcount = chunks[0][2]
                        if chunkcount == 1:
                            fullrevisions.append(chunks[0])
                        elif chunkcount == len(chunks):
                            fullchunk = list(chunks[0])
                            data1 = b""
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

        def _updaterevisionreferences(self):
            reponame = self.sqlreponame
            cursor = self.sqlcursor

            # Compute new heads, and delete old heads
            newheads = set(hex(n) for n in self.heads())
            oldheads = []
            cursor.execute(
                "SELECT value FROM revision_references "
                "WHERE repo = %s AND namespace='heads'",
                (reponame,),
            )
            headsindb = cursor.fetchall()
            for head in headsindb:
                head = decodeutf8(head[0])
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
            newbookmarks = dict(
                (k, hex(v)) for k, v in pycompat.iteritems(self._bookmarks)
            )
            oldbookmarks = []
            cursor.execute(
                "SELECT name, value FROM revision_references "
                "WHERE namespace = 'bookmarks' AND repo = %s",
                (reponame,),
            )
            bookmarksindb = cursor.fetchall()
            for k, v in bookmarksindb:
                k = decodeutf8(k)
                v = decodeutf8(v)
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

            for k, v in pycompat.iteritems(newbookmarks):
                tmpl.append("(%s, 'bookmarks', %s, %s)")
                values.append(repo.sqlreponame)
                values.append(k)
                values.append(v)

            if tmpl:
                cursor.execute(
                    "INSERT INTO "
                    + "revision_references(repo, namespace, name, value) "
                    + "VALUES %s" % ",".join(tmpl),
                    tuple(values),
                )

            # revision_references has multiple keys (primary key, and a unique
            # index), so mysql gives a warning when using ON DUPLICATE KEY since
            # it would only update one row despite multiple key duplicates. This
            # doesn't matter for us, since we know there is only one row that
            # will share the same key. So suppress the warning.
            cursor.execute(
                "INSERT INTO revision_references(repo, namespace, name, value) "
                + "VALUES(%s, 'tip', 'tip', %s) "
                + "ON DUPLICATE KEY UPDATE value=%s",
                (reponame, len(self) - 1, len(self) - 1),
            )

        def committodb(self, tr):
            """Commits all pending revisions to the database"""
            if self.disablesync:
                return

            if self.sqlconn is None:
                raise util.Abort(
                    _("invalid repo change - only @prog@ push and pull are allowed")
                )

            if not self.pendingrevs and not "bookmark_moved" in tr.hookargs:
                return

            try:
                self._committodb(self.pendingrevs)

                # Just to be super sure, check the write lock before doing the
                # final commit
                if not self.hassqlwritelock():
                    raise Exception(
                        "attempting to write to sql "
                        + "without holding %s (precommit)" % writelock
                    )
                self.sqlconn.commit()
            except:
                self.sqlconn.rollback()
                raise
            finally:
                del self.pendingrevs[:]

        def _committodb(self, revisions, ignoreduplicates=False):
            if revisions:
                self._validatependingrevs(revisions, ignoreduplicates=ignoreduplicates)

            self._addrevstosql(revisions, ignoreduplicates=ignoreduplicates)
            self._updaterevisionreferences()

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
                chunkcount = datalen // maxrowsize
                if datalen % maxrowsize != 0 or datalen == 0:
                    chunkcount += 1

                if len(path) > 512:
                    raise util.Abort(
                        "invalid path '%s': paths must be not longer than 512 characters"
                        % path
                    )

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

        def _validatependingrevs(self, revisions, ignoreduplicates=False):
            """Validates that the current pending revisions will be valid when
            written to the database.
            """
            reponame = self.sqlreponame
            cursor = self.sqlcursor

            # Ensure we hold the write lock
            if not self.hassqlwritelock():
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
                    "multiple tips for %s in the database" % reponame
                )

            if (not ignoreduplicates) and (
                not util.safehasattr(self, "sqlreplaytransaction")
                or not self.sqlreplaytransaction
            ):
                minlinkrev = min(revisions, key=lambda x: x[1])[1]
                if maxlinkrev is None or maxlinkrev != minlinkrev - 1:
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
            pending = set([(path, rev) for path, _, rev, _, _, _, _ in revisions])
            expectedrevs = set()
            for revision in revisions:
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
                    path = decodeutf8(path)
                    node = decodeutf8(node)
                    rev = int(rev)
                    checkrevs.remove((path, rev))
                    rl = None
                    if path == "00changelog.i":
                        rl = self.changelog
                    elif path == "00manifest.i":
                        rl = self.manifestlog._revlog
                    else:
                        rl = revlog.revlog(self.svfs, path, mmaplargeindex=True)
                    localnode = hex(rl.node(rev))
                    if localnode != node:
                        raise CorruptionException(
                            "expected node %s at rev %d of %s but found %s"
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
            if self.hassqlwritelock(checkserver=False):
                self.sqlpostrelease.append(callback)
            else:
                return super(sqllocalrepo, self)._afterlock(callback)

    ui = repo.ui

    sqlargs = {}
    sqlargs["host"] = ui.config("hgsql", "host")
    sqlargs["database"] = ui.config("hgsql", "database")
    sqlargs["user"] = ui.config("hgsql", "user")
    sqlargs["port"] = ui.configint("hgsql", "port")
    sqlargs["connection_timeout"] = ui.configint("hgsql", "sockettimeout")
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

    class CustomConverter(mysql.connector.conversion.MySQLConverter):
        """Ensure that all values being returned are returned as bytes and not
        as strings."""

        def _STRING_to_python(self, value, dsc=None):
            return bytes(value)

        def _VAR_STRING_to_python(self, value, dsc=None):
            return bytes(value)

        def _BLOB_to_python(self, value, dsc=None):
            return bytes(value)

        def _bytearray_to_mysql(self, value, dsc=None):
            return bytes(value)

        def _memoryview_to_mysql(self, value, dsc=None):
            return value.tobytes()


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
            fp.write(b"".join(buffer))
            fp.close()

    def close(self):
        self.flush()
        self.closed = True


def addentries(repo, queue, transaction, ignoreexisting: bool = False) -> None:
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

    for filelog in pycompat.itervalues(revlogs):
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
            self.version &= ~(revlog.FLAG_GENERALDELTA | revlog.FLAG_INLINE_DATA)
            self._generaldelta = False
            self._inline = False

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


def addgroup(orig, self: Sized, deltas, linkmapper, transaction):
    """Copy paste of revlog.addgroup, but we ensure that the revisions are
    added in linkrev order.
    """
    if not util.safehasattr(transaction, "repo"):
        return orig(self, deltas, linkmapper, transaction)

    # track the base of the current delta log
    content = []
    node = None

    r = len(self)
    end = 0
    if r:
        # pyre-fixme[16]: `Sized` has no attribute `end`.
        end = self.end(r - 1)
    # pyre-fixme[16]: `Sized` has no attribute `opener`.
    # pyre-fixme[16]: `Sized` has no attribute `indexfile`.
    ifh = self.opener(self.indexfile, "a+")
    # pyre-fixme[16]: `Sized` has no attribute `_io`.
    isize = r * self._io.size
    # pyre-fixme[16]: `Sized` has no attribute `_inline`.
    if self._inline:
        transaction.add(self.indexfile, end + isize, r)
        dfh = None
    else:
        transaction.add(self.indexfile, isize, r)
        # pyre-fixme[16]: `Sized` has no attribute `datafile`.
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
            # pyre-fixme[16]: `Sized` has no attribute `node`.
            prevnode = self.node(len(self) - 1)
            for link, chunkdata in chunkdatas:
                node = chunkdata["node"]
                deltabase = chunkdata["deltabase"]
                # pyre-fixme[16]: `Sized` has no attribute `nodemap`.
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

            # pyre-fixme[16]: `Sized` has no attribute `rev`.
            baserev = self.rev(deltabase)
            # pyre-fixme[16]: `Sized` has no attribute `_addrevision`.
            self._addrevision(
                node, None, transaction, link, p1, p2, flags, (baserev, delta), ifh, dfh
            )

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
    "sqlrecover",
    [
        ("f", "force", "", _("strips as far back as necessary"), ""),
        ("", "no-backup", None, _("does not produce backup bundles for strips")),
    ],
    _("@prog@ sqlrecover"),
    norepo=True,
)
def sqlrecover(ui, *args, **opts) -> None:
    """
    Strips commits from the local repo until it is back in sync with the SQL
    server.
    """

    global initialsync
    initialsync = INITIAL_SYNC_DISABLE
    repo = hg.repository(ui, pycompat.getcwd())
    repo.disablesync = True

    if repo.recover():
        ui.status("recovered from incomplete transaction")

    def iscorrupt():
        repo.sqlconnect()
        try:
            repo.pullfromdb()
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
        ui.status("stripping back to %s commits" % striprev)

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
            "root-only",
            None,
            _("only strip the root tree manifest, not the sub-trees"),
        ),
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run sqltreestrip if you know exactly what you're doing"),
        ),
    ],
    _("@prog@ sqltreestrip REV"),
)
def sqltreestrip(ui, repo, rev: int, *args, **opts) -> Optional[int]:
    """Strips trees from local and sql history"""
    try:
        treemfmod = extensions.find("treemanifest")
    except KeyError:
        ui.warn(_("treemanifest is not enabled for this repository\n"))
        return 1

    if not repo.ui.configbool("treemanifest", "server"):
        ui.warn(_("this repository is not configured to be a treemanifest server\n"))
        return 1

    if not opts.get("i_know_what_i_am_doing"):
        raise util.Abort(
            "You must pass --i-know-what-i-am-doing to run this "
            + "command. If you have multiple servers using the database, this "
            + "command will break your servers until you run it on each one. "
            + "Only the Mercurial server admins should ever run this."
        )

    rootonly = opts.get("root_only")
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
        with repo.lock():
            repo.sqlconnect()
            repo.sqlwritelock()
            try:
                cursor = repo.sqlcursor

                if rootonly:
                    ui.status(
                        _("mysql: deleting root trees with linkrevs >= %s\n") % rev
                    )
                    pathfilter = "(path = '00manifesttree.i')"
                else:
                    ui.status(_("mysql: deleting trees with linkrevs >= %s\n") % rev)
                    pathfilter = "(path LIKE 'meta/%%' OR path = '00manifesttree.i')"

                cursor.execute(
                    """DELETE FROM revisions WHERE repo = %s AND linkrev >= %s
                       AND """
                    + pathfilter,
                    (reponame, rev),
                )
                repo.sqlconn.commit()
            finally:
                repo.sqlwriteunlock()
                repo.sqlclose()

    # strip from local
    with repo.wlock(), repo.lock(), repo.transaction("treestrip") as tr:
        repo.disablesync = True

        # Duplicating some logic from repair.py
        offset = len(tr.entries)
        tr.startgroup()
        if opts.get("root_only"):
            ui.status(_("local: deleting root trees with linkrevs >= %s\n") % rev)
            treerevlog = repo.manifestlog.treemanifestlog._revlog
            treerevlog.strip(rev, tr)
        else:
            ui.status(_("local: deleting trees with linkrevs >= %s\n") % rev)
            files = treemfmod.collectfiles(None, repo, rev)
            treemfmod.striptrees(None, repo, tr, rev, files)
        tr.endgroup()

        for i in range(offset, len(tr.entries)):
            file, troffset, ignore = tr.entries[i]
            with repo.svfs(file, "a", checkambig=True) as fp:
                util.truncate(fp, troffset)
            if troffset == 0:
                repo.store.markremoved(file)


def _parsecompressedrevision(data: bytes) -> Tuple[bytes, bytes]:
    """Takes a compressed revision and parses it into the data0 (compression
    indicator) and data1 (payload). Ideally we'd refactor revlog.decompress to
    have this logic be separate, but there are comments in the code about perf
    implications of the hotpath."""
    # The passed in value can be memoryviews or buffers, but we want to be able
    # to slice out bytes so we can compare them. Since this code is only used
    # for sqlrefill, let's just copy the bytes.
    if not isinstance(data, bytes):
        data = bytes(data)

    t = data[0:1]
    if t == b"u":
        return b"u", data[1:]
    else:
        return b"", data


def _discoverrevisions(repo, startrev):
    # Tuple of revlog name and rev number for revisions introduced by commits
    # greater than or equal to startrev (path, rlrev)
    revisions = []

    mfrevlog = repo.manifestlog._revlog
    for rev in repo.revs("%d:", startrev):
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
    _("@prog@ sqlrefill REV"),
    norepo=True,
)
def sqlrefill(ui, startrev: int, **opts) -> None:
    """Inserts the given revs into the database"""
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
        repo = hg.repository(ui, pycompat.getcwd())
        repo.disablesync = True

    startrev = int(startrev)

    with repo.lock():
        repo.sqlconnect()
        repo.sqlwritelock()
        try:
            revlogs = {}
            pendingrevs = []
            # totalrevs = len(repo.changelog)
            # with progress.bar(ui, 'refilling', total=totalrevs - startrev) as prog:
            # prog.value += 1
            # pyre-fixme[61]: `repo` is undefined, or not always defined.
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

                hexnode = hex(node)
                sqlentry = rl._io.packentry(entry, hexnode, rl.version, rlrev)
                revdata = (path, linkrev, rlrev, hexnode, sqlentry, data0, data1)
                pendingrevs.append(revdata)

            repo._committodb(pendingrevs, ignoreduplicates=True)
            repo.sqlconn.commit()
        finally:
            repo.sqlwriteunlock()
            repo.sqlclose()


@command(
    "sqlstrip",
    [
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run sqlstrip if you know exactly what you're doing"),
        ),
        (
            "",
            "no-backup-permanent-data-loss",
            None,
            _("does not produce backup bundles (for use with corrupt revlogs)"),
        ),
    ],
    _("@prog@ sqlstrip [OPTIONS] REV"),
    norepo=True,
)
def sqlstrip(ui, rev: int, *args, **opts) -> None:
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
    repo = hg.repository(ui, pycompat.getcwd())
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
            repo.sqlwritelock()

            if rev not in repo:
                raise util.Abort("revision %s is not in the repo" % rev)

            reponame = repo.sqlreponame
            cursor = repo.sqlcursor
            changelog = repo.changelog

            revs = repo.revs("%d:" % rev)
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
            repo._updaterevisionreferences()

            ui.status("deleting revision data\n")
            cursor.execute(
                """DELETE FROM revisions WHERE repo = %s and linkrev >= %s""",
                (reponame, rev),
            )

            repo.sqlconn.commit()
        finally:
            repo.sqlwriteunlock()
            repo.sqlclose()
    finally:
        if lock:
            lock.release()
        if wlock:
            wlock.release()


@command(
    "sqlreplay",
    [
        ("", "start", "", _("the rev to start with"), ""),
        ("", "end", "", _("the rev to end with"), ""),
    ],
    _("@prog@ sqlreplay"),
)
def sqlreplay(ui, repo, *args, **opts) -> None:
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


def _sqlreplay(repo, startrev, endrev) -> None:
    wlock = lock = None

    try:
        wlock = repo.wlock()
        lock = repo.lock()
        # Disable all pretxnclose hooks, since these revisions are
        # technically already committed.
        for name, value in repo.ui.configitems("hooks"):
            if name.startswith("pretxnclose"):
                repo.ui.setconfig("hooks", name, None)

        transaction = repo.transaction("sqlreplay")

        try:
            repo.sqlreplaytransaction = True
            q = queue.Queue()
            abort = threading.Event()

            t = threading.Thread(
                target=repo.fetchthread, args=(q, abort, startrev, endrev)
            )
            t.setDaemon(True)
            try:
                t.start()
                addentries(repo, q, transaction, ignoreexisting=True)
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
    "sqlverify",
    [("", "earliest-rev", "", _("the earliest rev to process"), "")],
    _("@prog@ sqlverify"),
)
def sqlverify(ui, repo, *args, **opts) -> None:
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
                        rl = repo.changelog
                    elif filepath == "00manifest.i":
                        rl = repo.manifestlog._revlog
                    else:
                        rl = revlog.revlog(repo.svfs, filepath)
                for rev in range(len(rl) - 1, -1, -1):
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
    q = queue.Queue()
    abort = threading.Event()
    t = threading.Thread(target=repo.fetchthread, args=(q, abort, minrev, maxrev))
    t.setDaemon(True)

    insql = set()
    try:
        t.start()

        while True:
            revdata = q.get()
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
                    rl = repo.changelog
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
                    "'%s:%s' with linkrev %s, disk does not match mysql"
                    % (path, hex(node), str(linkrev))
                )
    finally:
        abort.set()
