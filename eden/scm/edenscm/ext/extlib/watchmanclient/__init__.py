# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# __init__.py - Watchman client for the fsmonitor extension

from __future__ import absolute_import

import ctypes
import getpass
import os
import sys
import time

from edenscm import blackbox, encoding, json, progress, pycompat, util
from edenscm.i18n import _
from edenscm.node import hex

from .. import pywatchman
from ..pywatchman import compat


def createclientforrepo(repo):
    """Creates a Watchman client and associates it with the repo if it does
    not already have one. Note that creating the client may raise an exception.

    To get the client associated with the repo, use getclientforrepo()."""
    if not util.safehasattr(repo, "_watchmanclient"):
        repo._watchmanclient = client(repo)


def getclientforrepo(repo):
    """Returns the Watchman client associated with the repo or None.

    createclientforrepo() must have be called previously to create the
    client."""
    if util.safehasattr(repo, "_watchmanclient"):
        return repo._watchmanclient
    else:
        return None


class Unavailable(Exception):
    def __init__(self, msg, warn=True, invalidate=False):
        self.msg = msg
        self.warn = warn
        if self.msg == "timed out waiting for response":
            self.warn = False
        self.invalidate = invalidate

    def __str__(self):
        if self.warn:
            return "warning: Watchman unavailable: %s" % self.msg
        else:
            return "Watchman unavailable: %s" % self.msg


class WatchmanNoRoot(Unavailable):
    def __init__(self, root, msg):
        self.root = root
        super(WatchmanNoRoot, self).__init__(msg)


class client(object):
    def __init__(self, repo, timeout=1.0):
        err = None
        if not self._user:
            err = "couldn't get user"
            warn = True
        if self._user in repo.ui.configlist("fsmonitor", "blacklistusers"):
            err = "user %s in blacklist" % self._user
            warn = False

        if err:
            raise Unavailable(err, warn)

        self._sockpath = None

        # When spawned indirectly by watchman, or the watchman/eden integration
        # tests, the appropriate sockpath is passed down to us via the environment
        # and must take precedence over other configuration
        sockpath = encoding.environ.get("WATCHMAN_SOCK", None)
        if sockpath is None:
            sockpath = repo.ui.config("fsmonitor", "sockpath")
            if sockpath and self._user:
                sockpath = sockpath.replace("%i", self._user)
                repo.ui.debug("watchman sockpath is set as %s\n" % sockpath)

        if sockpath:
            if os.path.exists(sockpath):
                self._sockpath = sockpath

        self._transport = None
        if repo.ui.configbool("fsmonitor", "tcp", False):
            self._transport = "tcp"
        self._tcp_host = repo.ui.config("fsmonitor", "tcp-host", "::1")
        self._tcp_port = repo.ui.configint("fsmonitor", "tcp-port", 12300)
        self._timeout = timeout
        self._watchmanclient = None
        self._root = repo.root
        use_ctypes = repo.ui.configbool("fsmonitor", "canonical-path-ctypes")
        self._resolved_root = getcanonicalpath(self._root, use_ctypes)
        self._ui = repo.ui
        self._firsttime = True
        try:
            self._approx_total_file_count = len(repo.dirstate._map)
        except Exception:
            # EdenFS-based dirstates don't include all the files. They also
            # don't crawl the repo, so this value isn't important anymore.
            self._approx_total_file_count = 0

    def settimeout(self, timeout):
        self._timeout = timeout
        if self._watchmanclient is not None:
            self._watchmanclient.setTimeout(timeout)

    def getcurrentclock(self):
        result = self.command("clock")
        if not util.safehasattr(result, "clock"):
            raise Unavailable("clock result is missing clock value", invalidate=True)
        return result.clock

    def clearconnection(self):
        self._watchmanclient = None

    def available(self):
        return self._watchmanclient is not None or self._firsttime

    @util.propertycache
    def _user(self):
        try:
            return getpass.getuser()
        except KeyError:
            # couldn't figure out our user
            return None

    def _command(self, *args):
        with util.traced("watchman-command", args=json.dumps(args[1:])) as span:
            return self._retrycommand(span, 0, *args)

    def _retrycommand(self, span, retry, *args):
        if retry > 0:
            span.record(retry=retry)
        watchmanargs = (args[0], self._resolved_root) + args[1:]
        error = None
        needretry = False
        starttime = util.timer()
        try:
            if self._watchmanclient is None:
                if compat.PYTHON3:
                    encoding = "bser"
                else:
                    encoding = "bser-v1"
                self._firsttime = False
                self._watchmanclient = pywatchman.client(
                    sockpath=self._sockpath,
                    transport=self._transport,
                    tcpAddress=(self._tcp_host, self._tcp_port),
                    timeout=self._timeout,
                    recvEncoding=encoding,
                    sendEncoding=encoding,
                    useImmutableBser=True,
                )
            return self._watchmanclient.query(*watchmanargs)
        except pywatchman.CommandError as ex:
            error = ex.msg
            span.record(error=ex.msg)
            if "unable to resolve root" in ex.msg:
                raise WatchmanNoRoot(self._resolved_root, ex.msg)
            raise Unavailable(ex.msg)
        except pywatchman.SocketConnectError as ex:
            error = str(ex)
            # If fsmonitor.sockpath was specified in the configuration, we will
            # have skipped running `watchman get-sockname` which has the
            # consequence of not starting the watchman server up if it happens
            # to have been stopped.
            # Rather than just throwing up our hands in that situation, let's
            # clear the pre-configured sockpath so that the client will probe
            # and start it up.
            if not self._ui.config("fsmonitor", "sockpath") or self._sockpath is None:
                span.record(error=error)
                # Either sockpath wasn't configured, or we already tried clearing
                # it out, so let's propagate this error.
                raise Unavailable(str(ex))
            # Recurse and retry the command, and hopefully it will
            # start the server this time.
            self._sockpath = None
            self._watchmanclient = None
            needretry = True
        except pywatchman.WatchmanError as ex:
            error = str(ex)
            span.record(error=error)
            raise Unavailable(str(ex))
        finally:
            event = {
                "watchman": {
                    "args": args,
                    "duration_ms": int((util.timer() - starttime) * 1000),
                }
            }
            if error is not None:
                event["watchman"]["result"] = {"error": error}
            blackbox.log(event)
        if needretry:
            return self._retrycommand(span, retry + 1, *args)

    def debug_status(self):
        """Return the RootDebugStatus, which might look like:

            {
                "recrawl_info": {
                    "count": 0,
                    "should-recrawl": true,
                    "warning": null,
                    "reason": "startup",
                    "completed": null,
                    "started": -20162,
                    "stats": 295541
                }
                "crawl-status": "crawling for ...",
                "enable_parallel_crawl": false,
                "cookie_list": [],
                "path": "...",
                "queries": [],
                "fstype": "btrfs",
                "cookie_prefix": ["..."],
                "watcher": "inotify",
                "uptime": ...,
                "done_initial": false,
                "cookie_dir": [".../.hg"],
                "case_sensitive": true,
                "cancelled": false,
            }

        Return an empty dict if watchman does not support debug-root-status.
        """
        try:
            # use _command to bypass progress and util.timefuntion
            root_status = self._command("debug-root-status")["root_status"]
        except (Unavailable, KeyError):
            # watchman does not support this command
            root_status = {}
        # log enable_parallel_crawl
        try:
            enable_parallel_crawl = root_status["enable_parallel_crawl"]
        except KeyError:
            pass
        else:
            self._ui.log("fsmonitor_status", parallel_crawl=enable_parallel_crawl)
        return root_status

    def recrawl_info(self):
        """Return the RootRecrawlInfo in the RootDebugStatus.

        Return an empty dict if watchman does not support getting the recrawl
        info.
        """
        debug_status = self.debug_status()
        info = {}
        try:
            info = debug_status["recrawl_info"]
        except KeyError:
            pass
        return info

    def recrawl_stat_count(self):
        """Return the count of files stat()-ed by watchman during a full crawl
        Return None if watchman does not provide the information, or watchman
        is not in a full crawl state.
        """
        stats = None
        try:
            stats = self.recrawl_info()["stats"]
        except KeyError:
            pass
        return stats

    def wait_for_full_crawl(self):
        """Wait for watchman to complete a full recrawl. Blocking.
        Show a progress bar.
        """
        ui = self._ui
        if not ui.configbool("fsmonitor", "wait-full-crawl"):
            return

        stats = self.recrawl_stat_count()
        if stats is None:
            # Not in full recrawl
            return

        # Show progress bar.
        total = self._approx_total_file_count
        with progress.bar(ui, _("crawling"), _("files (approx)"), total) as prog:
            while stats is not None:
                prog.value = stats
                stats = self.recrawl_stat_count()
                if stats is not None:
                    time.sleep(0.1)

    @util.timefunction("watchmanquery", 0, "_ui")
    def command(self, *args, **kwargs):
        ignoreerrors = kwargs.get("ignoreerrors", False)
        if args and args[0] in {"clock", "query"}:
            self.wait_for_full_crawl()
        with progress.spinner(self._ui, "querying watchman"):
            try:
                try:
                    return self._command(*args)
                except pywatchman.UseAfterFork:
                    # Ideally we wouldn't let this happen, but if it does happen,
                    # record it in the log and retry the command.
                    blackbox.log(
                        {
                            "debug": {
                                "value": "fork detected. re-connect to watchman socket"
                            }
                        }
                    )
                    self._watchmanclient = None
                    return self._command(*args)
                except WatchmanNoRoot:
                    # this 'watch' command can also raise a WatchmanNoRoot if
                    # watchman refuses to accept this root
                    self._command("watch")
                    return self._command(*args)
            except Unavailable:
                # this is in an outer scope to catch Unavailable form any of the
                # above _command calls
                if not ignoreerrors:
                    self._watchmanclient = None
                raise


# Estimate the distance between two nodes
def calcdistance(repo, oldnode, newnode):
    cl = repo.changelog
    if cl.algorithmbackend == "segments":
        only = cl.dag.only
        distance1 = len(only([oldnode], [newnode]))
        distance2 = len(only([newnode], [oldnode]))
        distance = distance1 + distance2
    else:
        anc = repo.changelog.ancestor(oldnode, newnode)
        ancrev = repo[anc].rev()
        distance = abs(repo[oldnode].rev() - ancrev) + abs(repo[newnode].rev() - ancrev)
    return distance


class state_update(object):
    """This context manager is responsible for dispatching the state-enter
    and state-leave signals to the watchman service. The enter and leave
    methods can be invoked manually (for scenarios where context manager
    semantics are not possible). If parameters oldnode and newnode are None,
    they will be populated based on current working copy in enter and
    leave, respectively. Similarly, if the distance is none, it will be
    calculated based on the oldnode and newnode in the leave method."""

    def __init__(
        self,
        repo,
        name,
        oldnode=None,
        newnode=None,
        distance=None,
        partial=False,
        metadata=None,
    ):
        self.repo = repo
        self.name = name
        self.oldnode = oldnode
        self.newnode = newnode
        self.distance = distance
        self.partial = partial
        self._lock = None
        self.need_leave = False
        self.metadata = metadata or {}

    def __enter__(self):
        self.enter()

    def enter(self):
        # Make sure we have a wlock prior to sending notifications to watchman.
        # We don't want to race with other actors. In the update case,
        # merge.update is going to take the wlock almost immediately. We are
        # effectively extending the lock around several short sanity checks.
        if self.oldnode is None:
            self.oldnode = self.repo["."].node()

        if self.repo.currentwlock() is None:
            if util.safehasattr(self.repo, "wlocknostateupdate"):
                self._lock = self.repo.wlocknostateupdate()
            else:
                self._lock = self.repo.wlock()
        self.need_leave = self._state("state-enter", hex(self.oldnode))
        return self

    def __exit__(self, type_, value, tb):
        abort = True if type_ else False
        self.exit(abort=abort)

    def exit(self, abort=False):
        try:
            if self.need_leave:
                status = "failed" if abort else "ok"
                if self.newnode is None:
                    self.newnode = self.repo["."].node()
                if self.distance is None:
                    try:
                        self.distance = calcdistance(
                            self.repo, self.oldnode, self.newnode
                        )
                    except Exception:
                        # this happens in complex cases where oldnode
                        # or newnode might become unavailable.
                        pass
                self._state("state-leave", hex(self.newnode), status=status)
        finally:
            self.need_leave = False
            if self._lock:
                self._lock.release()

    def _state(self, cmd, commithash, status="ok"):
        client = getclientforrepo(self.repo)
        if not client:
            return False
        try:
            metadata = {
                # the target revision
                "rev": commithash,
                # approximate number of commits between current and target
                "distance": self.distance if self.distance else 0,
                # success/failure (only really meaningful for state-leave)
                "status": status,
                # whether the working copy parent is changing
                "partial": self.partial,
            }
            metadata.update(self.metadata)
            client.command(
                # ignoreerrors=True prevents the client from invalidating it's
                # watchman client in the event of an error. It will still throw
                # an exception though.
                cmd,
                {"name": self.name, "metadata": metadata},
                ignoreerrors=True,
            )
            return True
        except Exception as ex:
            # Swallow any errors; fire and forget
            exctype = sys.exc_info()[0]
            exctypename = "None" if exctype is None else exctype.__name__
            self.repo.ui.log(
                "hgerrors",
                "watchman '%s' event has failed: %s",
                cmd,
                str(ex),
                exception_type=exctypename,
            )
            return False


if pycompat.iswindows:
    from ctypes.wintypes import DWORD, HANDLE

    def openfilewin(path):

        createfile = ctypes.windll.kernel32.CreateFileW

        cpath = ctypes.create_unicode_buffer(path)
        access = 0
        mode = 7  # FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE
        disposition = 3  # OPEN_EXISTING
        flags = 33554432  # FILE_FLAG_BACKUP_SEMANTICS

        createfile.restype = HANDLE
        h = createfile(
            cpath,
            DWORD(access),
            DWORD(mode),
            None,
            DWORD(disposition),
            DWORD(flags),
            HANDLE(0),
        )
        if h == HANDLE(-1).value:
            raise WindowsError("Failed to open file: " + path)

        return HANDLE(h)

    def _getcanonicalpath(name):
        gfpnbh = ctypes.windll.kernel32.GetFinalPathNameByHandleW
        closehandler = ctypes.windll.kernel32.CloseHandle

        h = openfilewin(name)
        try:
            numwchars = 1024
            while True:
                buf = ctypes.create_unicode_buffer(numwchars)
                result = gfpnbh(h, buf, DWORD(numwchars), DWORD(0))
                if result == 0:
                    raise IOError("unknown error while normalizing path")

                # The first four chars are //?/
                if result <= numwchars:
                    path = buf.value[4:].replace("\\", "/")
                    if compat.PYTHON2:
                        path = path.encode("utf-8")
                    return path

                # Not big enough; the result is the amount we need
                numwchars = result + 1
        finally:
            closehandler(h)

elif pycompat.isdarwin:
    import ctypes.util

    F_GETPATH = 50
    libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
    getpathfcntl = libc.fcntl
    getpathfcntl.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_char_p]
    getpathfcntl.restype = ctypes.c_int

    def _getcanonicalpath(name):
        fd = os.open(name, os.O_RDONLY, 0)
        try:
            numchars = 1024  # MAXPATHLEN
            # The kernel caps this routine to MAXPATHLEN, so there is no
            # point in over-allocating or trying again with a larger buffer
            buf = ctypes.create_string_buffer(numchars)
            ctypes.set_errno(0)
            result = getpathfcntl(fd, F_GETPATH, buf)
            if result != 0:
                raise OSError(ctypes.get_errno())
            # buf is a bytes buffer, so normalize it if necessary
            ret = buf.value
            if isinstance(name, compat.UNICODE):
                ret = os.fsdecode(ret)
            return ret
        finally:
            os.close(fd)

else:

    def _getcanonicalpath(name):
        return os.path.normpath(name)


def getcanonicalpath(name, use_ctypes=False):
    if use_ctypes:
        return _getcanonicalpath(name)

    import bindings

    canonical = bindings.fs.canonicalize(name)

    # Match the old code behavior, not sure if necessary or correct.
    if os.name == "nt":
        canonical = canonical[4:].replace("\\", "/")

    return canonical
