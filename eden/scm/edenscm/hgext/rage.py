# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""upload useful diagnostics and give instructions for asking for help

    [rage]
    # Name of the rpm binary
    rpmbin = rpm
"""
import ctypes
import datetime
import glob
import json
import os
import re
import socket
import struct
import subprocess
import tempfile
import threading
import time
import traceback
from functools import partial

from edenscm.mercurial import (
    bookmarks,
    cmdutil,
    commands,
    encoding,
    error,
    progress,
    pycompat,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

from .remotefilelog import constants, shallowutil


cmdtable = {}
command = registrar.command(cmdtable)

BLACKBOX_PATTERN = """
["or",
 {"legacy_log":
  {"msg":"_",
   "service": ["not", ["or", "remotefilelog", "remotefilefetchlog"]]}},
 ["not", {"legacy_log": "_"}]]
"""


def shcmd(cmd, input=None, check=True, keeperr=True):
    _, _, _, p = util.popen4(cmd)
    out, err = p.communicate(input)
    if check and p.returncode:
        raise error.Abort(cmd + " error: " + err)
    elif keeperr:
        out += err
    return out


def which(name):
    """ """
    for p in encoding.environ.get("PATH", "/bin").split(pycompat.ospathsep):
        path = os.path.join(p, name)
        if os.path.exists(path):
            return path
    return None


def _tail(userlogdir, userlogfiles, nlines=100):
    """
    Returns the last `nlines` from logfiles
    """
    # create list of files (full paths)
    logfiles = [os.path.join(userlogdir, f) for f in userlogfiles]
    # sort by creation time
    logfiles = sorted(filter(os.path.isfile, logfiles), key=os.path.getmtime)
    # reverse to get from the latest
    logfiles = reversed(logfiles)
    logs = []
    # traverse the files
    linelimit = nlines
    for logfile in logfiles:
        loglines = open(logfile).readlines()
        linecount = len(loglines)
        if linecount > linelimit:
            logcontent = "  ".join(loglines[-linelimit:])
            logs.append(
                "%s (first %s lines omitted):\n\n  %s\n"
                % (logfile, linecount - linelimit, logcontent)
            )
            break
        else:
            logcontent = "  ".join(loglines)
            logs.append("%s:\n\n  %s\n" % (logfile, logcontent))
            linelimit -= linecount
    return "".join(reversed(logs))


rageopts = [
    ("p", "preview", None, _("print diagnostic information without uploading paste")),
    ("t", "timeout", 20, _("maximum seconds spent on collecting one section")),
]


def localconfig(ui):
    result = []
    for section, name, value in ui.walkconfig():
        source = ui.configsource(section, name)
        if source.find("/etc/") == -1 and source.find("builtin") == -1:
            result.append("%s.%s=%s  # %s" % (section, name, value, source))
    return result


def overriddenconfig(ui):
    result = []
    for section, name, value in ui.walkconfig():
        source = ui.configsource(section, name)
        if source.find("overrides") > -1:
            result.append("%s.%s=%s  # %s" % (section, name, value, source))
    return result


def usechginfo():
    """FBONLY: Information about whether chg is enabled"""
    files = {"system": "/etc/mercurial/usechg", "user": os.path.expanduser("~/.usechg")}
    result = []
    for name, path in files.items():
        if os.path.exists(path):
            with open(path) as f:
                value = f.read().strip()
        else:
            value = "(not set)"
        result.append("%s: %s" % (name, value))
    return "\n".join(result)


def rpminfo(ui):
    """FBONLY: Information about RPM packages"""
    result = set()
    rpmbin = ui.config("rage", "rpmbin", "rpm")
    for name in ["hg", "hg.real"]:
        path = which(name)
        if not path:
            continue
        result.add(shcmd("%s -qf %s" % (rpmbin, path), check=False))
    return "".join(result)


def infinitepushbackuplogs(ui, repo):
    """Contents of recent infinitepush log files."""
    logdir = ui.config("infinitepushbackup", "logdir")
    if not logdir:
        return "infinitepushbackup.logdir not set"
    try:
        # the user name from the machine
        username = util.getuser()
    except Exception:
        username = "unknown"

    userlogdir = os.path.join(logdir, username)
    if not os.path.exists(userlogdir):
        return "log directory does not exist: %s" % userlogdir

    reponame = os.path.basename(repo.origroot)
    logfiles = [f for f in os.listdir(userlogdir) if f[:-8] == reponame]

    if not logfiles:
        return "no log files found for %s in %s" % (reponame, userlogdir)

    return _tail(userlogdir, logfiles, 100)


def scmdaemonlog(ui, repo):
    logpath = ui.config("commitcloud", "scm_daemon_log_path")

    if not logpath:
        return "'commitcloud.scm_daemon_log_path' is not set in the config"

    logpath = util.expanduserpath(logpath)

    if not os.path.exists(logpath):
        return "%s: no such file or directory" % logpath

    # grab similar files as the original path to include rotated logs as well
    logfiles = [
        f
        for f in os.listdir(os.path.dirname(logpath))
        if os.path.basename(logpath) in f
    ]
    return _tail(os.path.dirname(logpath), logfiles, 150)


def readinfinitepushbackupstate(repo):
    dirname = "infinitepushbackups"
    if repo.sharedvfs.exists(dirname):
        result = []
        dir = repo.sharedvfs.join(dirname)
        for filename in os.listdir(dir):
            if "infinitepushbackupstate" in filename:
                with open(os.path.join(dir, filename), "r") as f:
                    result.append("reading infinitepush state file: %s" % filename)
                    result.append(json.dumps(json.load(f), indent=4))
        return "\n".join(result)
    else:
        return "no any infinitepushbackupstate file in the repo\n"


def readcommitcloudstate(repo):
    prefixpath = repo.svfs.join("commitcloudstate")
    files = glob.glob(prefixpath + "*")
    if not files:
        return "no any commitcloudstate file in the repo\n"
    lines = []
    for filename in files:
        lines.append("reading commit cloud workspace state file: %s" % filename)
        with open(filename, "r") as f:
            lines.append(json.dumps(json.load(f), indent=4))
    return "\n".join(lines) + "\n"


def _makerage(ui, repo, **opts):
    # Make graphlog shorter.
    configoverrides = {("experimental", "graphshorten"): "1"}

    def hgcmd(cmdname, *args, **additional_opts):
        cmd, opts = cmdutil.getcmdanddefaultopts(cmdname, commands.table)
        opts.update(additional_opts)

        _repo = repo
        if "_repo" in opts:
            _repo = opts["_repo"]
            del opts["_repo"]
        # If we failed to popbuffer for some reason, do not mess up with the
        # main `ui` object.
        newui = ui.copy()
        newui.pushbuffer(error=True)
        try:
            with ui.configoverride(configoverrides, "rage"):
                if cmd.norepo:
                    cmd(newui, *args, **opts)
                else:
                    cmd(newui, _repo, *args, **opts)
        finally:
            return newui.popbuffer()

    basic = [
        ("date", lambda: time.ctime()),
        ("unixname", lambda: encoding.environ.get("LOGNAME")),
        ("hostname", lambda: socket.gethostname()),
        ("repo location", lambda: repo.root),
        ("cwd", lambda: pycompat.getcwd()),
        ("fstype", lambda: util.getfstype(repo.root)),
        ("active bookmark", lambda: bookmarks._readactive(repo, repo._bookmarks)),
        (
            "hg version",
            lambda: __import__(
                "edenscm.mercurial.__version__"
            ).mercurial.__version__.version,
        ),
        ("obsstore size", lambda: str(repo.svfs.stat("obsstore").st_size)),
    ]

    oldcolormode = ui._colormode
    ui._colormode = None

    detailed = [
        ("df -h", lambda: shcmd("df -h", check=False)),
        # smartlog as the user sees it
        ("hg sl", lambda: hgcmd("smartlog", template="{sl_debug}")),
        # unfiltered smartlog for recent hidden changesets, including full
        # node identity
        (
            "hg sl --master='interestingmaster()' -r 'predecessors(draft())'",
            lambda: hgcmd(
                "smartlog",
                master="interestingmaster()",
                rev=["predecessors(draft())"],
                _repo=repo.unfiltered(),
                template='{sub("\\n", " ", "{node} {sl_debug}")}',
            ),
        ),
        (
            'first 20 lines of "hg status"',
            lambda: "\n".join(hgcmd("status").splitlines()[:20]),
        ),
        (
            "hg blackbox",
            lambda: "\n".join(
                hgcmd("blackbox", pattern=BLACKBOX_PATTERN).splitlines()[-500:]
            ),
        ),
        ("hg summary", lambda: hgcmd("summary")),
        ("hg cloud status", lambda: hgcmd("cloud status")),
        ("hg debugprocesstree", lambda: hgcmd("debugprocesstree")),
        ("hg config (local)", lambda: "\n".join(localconfig(ui))),
        ("hg sparse show", lambda: hgcmd("sparse show")),
        ("hg debuginstall", lambda: hgcmd("debuginstall")),
        ("usechg", (usechginfo)),
        (
            "uptime",
            lambda: shcmd(
                "wmic path Win32_OperatingSystem get LastBootUpTime"
                if pycompat.iswindows
                else "uptime"
            ),
        ),
        ("rpm info", (partial(rpminfo, ui))),
        ("klist", lambda: shcmd("klist", check=False)),
        ("ifconfig", lambda: shcmd("ipconfig" if pycompat.iswindows else "ifconfig")),
        (
            "airport",
            lambda: shcmd(
                "/System/Library/PrivateFrameworks/Apple80211."
                + "framework/Versions/Current/Resources/airport "
                + "--getinfo",
                check=False,
            ),
        ),
        (
            'last 100 lines of "hg debugobsolete"',
            lambda: "\n".join(hgcmd("debugobsolete").splitlines()[-100:]),
        ),
        ("infinitepush backup state", lambda: readinfinitepushbackupstate(repo)),
        ("commit cloud workspace sync state", lambda: readcommitcloudstate(repo)),
        (
            "infinitepush / commitcloud backup logs",
            lambda: infinitepushbackuplogs(ui, repo),
        ),
        ("scm daemon logs", lambda: scmdaemonlog(ui, repo)),
        ("debugstatus", lambda: hgcmd("debugstatus")),
        ("debugtree", lambda: hgcmd("debugtree")),
        ("hg config (overrides)", lambda: "\n".join(overriddenconfig(ui))),
        ("edenfs rage", lambda: shcmd("edenfsctl rage --stdout")),
        (
            "environment variables",
            lambda: "\n".join(
                sorted(["{}={}".format(k, v) for k, v in encoding.environ.items()])
            ),
        ),
        ("ssh config", lambda: shcmd("ssh -G hg.vip.facebook.com", check=False)),
    ]

    msg = ""

    if util.safehasattr(repo, "name"):
        # Add the contents of both local and shared pack directories.
        packlocs = {
            "local": lambda category: shallowutil.getlocalpackpath(
                repo.svfs.vfs.base, category
            ),
            "shared": lambda category: shallowutil.getcachepackpath(repo, category),
        }

        for loc, getpath in packlocs.iteritems():
            for category in constants.ALL_CATEGORIES:
                path = getpath(category)
                detailed.append(
                    (
                        "%s packs (%s)" % (loc, constants.getunits(category)),
                        lambda path=path: "%s:\n%s"
                        % (
                            path,
                            shcmd(
                                "dir /o-s %s" % os.path.normpath(path)
                                if pycompat.iswindows
                                else "ls -lhS %s" % path
                            ),
                        ),
                    )
                )

    footnotes = []
    timeout = opts.get("timeout") or 20

    def _failsafe(gen, timeout=timeout):
        class TimedOut(RuntimeError):
            pass

        def target(result, gen):
            try:
                result.append(gen())
            except TimedOut:
                return
            except Exception as ex:
                index = len(footnotes) + 1
                footnotes.append(
                    "[%d]: %s\n%s\n\n" % (index, str(ex), traceback.format_exc())
                )
                result.append("(Failed. See footnote [%d])" % index)

        result = []
        thread = threading.Thread(target=target, args=(result, gen))
        thread.daemon = True
        thread.start()
        thread.join(timeout)
        if result:
            value = result[0]
            return value
        else:
            if thread.is_alive():
                # Attempt to stop the thread, since hg is not thread safe.
                # There is no pure Python API to interrupt a thread.
                # But CPython C API can do that.
                ctypes.pythonapi.PyThreadState_SetAsyncExc(
                    ctypes.c_long(thread.ident), ctypes.py_object(TimedOut)
                )
            return (
                "(Did not complete in %s seconds, rerun with a larger --timeout to collect this)"
                % timeout
            )

    msg = []
    profile = []
    allstart = time.time()
    for name, gen in basic:
        msg.append("%s: %s\n\n" % (name, _failsafe(gen)))
    profile.append((time.time() - allstart, "basic info", None))
    for name, gen in detailed:
        start = time.time()
        with progress.spinner(ui, "collecting %r" % name):
            value = _failsafe(gen)
        finish = time.time()
        msg.append(
            "%s: (%.2f s)\n---------------------------\n%s\n\n"
            % (name, finish - start, value)
        )
        profile.append((finish - start, name, value.count("\n")))
    allfinish = time.time()
    profile.append((allfinish - allstart, "total time", None))

    msg.append("hg rage profile:\n")
    width = max([len(name) for _t, name, _l in profile])
    for timetaken, name, lines in reversed(sorted(profile)):
        m = "  %-*s  %8.2f s" % (width + 1, name + ":", timetaken)
        if lines is not None:
            msg.append("%s for %4d lines\n" % (m, lines))
        else:
            msg.append("%s\n" % m)
    msg.append("\n")

    msg.extend(footnotes)
    msg = "".join(msg)

    ui._colormode = oldcolormode
    return msg


@command("rage", rageopts, _("hg rage"))
def rage(ui, repo, *pats, **opts):
    """collect troubleshooting diagnostics

    The rage command collects useful diagnostic information.

    By default, the information will be uploaded to Phabricator and
    instructions about how to ask for help will be printed.

    After submitting to Phabricator, it prints configerable advice::

        [rage]
        advice = Please see our FAQ guide: https://...

    """
    with progress.spinner(ui, "collecting information"):
        msg = _makerage(ui, repo, **opts)

    if opts.get("preview"):
        ui.pager("rage")
        ui.write("%s\n" % msg)
        return

    with progress.spinner(ui, "saving paste"):
        try:
            p = subprocess.Popen(
                ["pastry", "--lang", "hgrage", "--title", "hgrage"],
                stdout=subprocess.PIPE,
                stdin=subprocess.PIPE,
                stderr=subprocess.PIPE,
                shell=pycompat.iswindows,
            )
            out, err = p.communicate(input=msg + "\n")
            ret = p.returncode
        except OSError:
            ui.write(_("Failed calling pastry. (is it in your PATH?)\n"))
            ret = 1

    if ret:
        fd, tmpname = tempfile.mkstemp(prefix="hg-rage-")
        with util.fdopen(fd, r"w") as tmpfp:
            tmpfp.write(msg)
            ui.write(
                _(
                    "Failed to post the diagnostic paste to Phabricator, "
                    "but its contents have been written to:\n\n"
                )
            )
            ui.write(_("  %s\n") % tmpname, label="rage.link")
            ui.write(
                _("\nPlease include this file in the %s.\n")
                % ui.config("ui", "supportcontact")
            )
    else:
        ui.write(
            _("Please post in %s with the following link:\n\n")
            % (ui.config("ui", "supportcontact"))
        )
        ui.write("  " + out + "\n", label="rage.link")
    ui.write(ui.config("rage", "advice", "") + "\n")


colortable = {"rage.link": "blue bold"}
