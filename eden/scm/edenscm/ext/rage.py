# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""upload useful diagnostics and give instructions for asking for help

    [rage]
    # Name of the rpm binary
    rpmbin = rpm
"""
import ctypes
import glob
import json
import os
import socket
import subprocess
import tempfile
import threading
import time
import traceback
from functools import partial
from pathlib import Path
from typing import List, Optional, Tuple

import bindings
from edenscm import (
    bookmarks,
    color,
    encoding,
    error,
    hintutil,
    progress,
    pycompat,
    registrar,
    util,
)
from edenscm.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

BLACKBOX_PATTERN = """
["or",
 {"legacy_log":
  {"msg":"_",
   "service": ["not", ["or", "remotefilelog", "remotefilefetchlog"]]}},
 ["not", {"legacy_log": "_"}]]
"""


def shcmd(cmd, input=None, check: bool = True, keeperr: bool = True) -> str:
    _, _, _, p = util.popen4(cmd)
    out, err = p.communicate(input)
    out = pycompat.decodeutf8(out, errors="replace")
    err = pycompat.decodeutf8(err, errors="replace")
    if check and p.returncode:
        raise error.Abort(cmd + " error: " + err)
    elif keeperr:
        out += err
    return out


def which(name) -> Optional[str]:
    """ """
    for p in encoding.environ.get("PATH", "/bin").split(pycompat.ospathsep):
        path = os.path.join(p, name)
        if os.path.exists(path):
            return path
    return None


def _tail(
    userlogdir, userlogfiles, nlines: int = 100, compactpattern: Optional[str] = None
) -> str:
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
        with open(logfile) as f:
            loglines = f.readlines()
        if compactpattern:
            loglinescompact = []
            compactpatterncounter = False
            for line in loglines:
                if compactpattern not in line:
                    if compactpatterncounter:
                        loglinescompact.append(
                            "......................................... and %d similar lines\n"
                            % compactpatterncounter
                        )
                    loglinescompact.append(line)
                    compactpatterncounter = 0
                else:
                    if compactpatterncounter == 0:
                        loglinescompact.append(line)
                    compactpatterncounter = compactpatterncounter + 1
            loglines = loglinescompact
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


rageopts: List[Tuple[str, str, Optional[int], str]] = [
    ("p", "preview", None, _("print diagnostic information without uploading paste")),
    ("t", "timeout", 20, _("maximum seconds spent on collecting one section")),
]


def localconfig(ui) -> List[str]:
    result = []
    for section, name, value in ui.walkconfig():
        source = ui.configsource(section, name)
        if (
            source.find("/etc/") == -1
            and source.find("builtin") == -1
            and source.find("hgrc.dynamic") == -1
        ):
            result.append("%s.%s=%s  # %s" % (section, name, value, source))
    return result


def allconfig(ui) -> List[str]:
    result = []
    for section, name, value in ui.walkconfig():
        source = ui.configsource(section, name)
        if source.find("builtin") == -1:
            result.append("%s.%s=%s  # %s" % (section, name, value, source))
    return result


def usechginfo() -> str:
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


def rpminfo(ui) -> str:
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
    return _tail(
        os.path.dirname(logpath),
        logfiles,
        nlines=150,
        compactpattern="Subscription is alive",
    )


def readbackedupheads(repo) -> str:
    dirname = "commitcloud"
    if repo.sharedvfs.exists(dirname):
        result = []
        dir = repo.sharedvfs.join(dirname)
        for filename in os.listdir(dir):
            if filename.startswith("backedupheads"):
                with open(os.path.join(dir, filename), "r") as f:
                    result.append("reading backedupheads file: %s" % filename)
                    result.append(f.read())
        return "\n".join(result)
    else:
        return "no any backedupheads file in the repo\n"


def readcommitcloudstate(repo) -> str:
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


def readsigtraces(repo) -> str:
    vfs = repo.localvfs
    names = vfs.listdir("sigtrace")
    names.sort(key=lambda name: -vfs.stat("sigtrace/%s" % name).st_mtime)
    result = ""
    for name in names:
        # hg serve (non-forking commandserver) is used by emacsclient and
        # can produce very long but boring traces. Skip them.
        if "serve" in name:
            continue
        content = pycompat.decodeutf8(
            vfs.tryread("sigtrace/%s" % name), errors="replace"
        )
        result += "%s:\n%s\n\n" % (name, content.strip())
    return result


def checkproxyagentstate(ui) -> str:
    if not ui.config("auth_proxy", "x2pagentd"):
        return "Not enabled"

    x2prage = shcmd("x2pagentctl rage -n")

    return "x2pagentd rage:\n\n{}".format(x2prage)


def sksagentrage(ui) -> str:
    sksagentpath = ui.config("rage", "sks-agent-path")
    if not sksagentpath:
        return "Agent not configured for this platform"

    agentbinary = Path(sksagentpath)
    if not agentbinary.is_file():
        return f"Agent's binary not present in {sksagentpath}"

    status = shcmd(f"{sksagentpath} rage --stdout --verbose=false")

    return "sks-agent status:\n\n{}".format(status)


def _makerage(ui, repo, **opts) -> str:
    configoverrides = {
        # Make graphlog shorter.
        ("experimental", "graphshorten"): "1",
        # Force use of lines-square renderer, as the user's configuration may
        # not render properly in a text file.
        ("experimental", "graph.renderer"): "lines-square",
        # Reduce the amount of data used for debugnetwork speed tests to
        # increase the chance they complete within 20s.
        ("debugnetwork", "speed-test-download-size"): "4M",
        ("debugnetwork", "speed-test-upload-size"): "1M",
    }
    configargs = [f"--config={k[0]}.{k[1]}={v}" for k, v in configoverrides.items()]

    # Override the encoding to "UTF-8" to generate the rage in UTF-8.
    oldencoding = encoding.encoding
    oldencodingmode = encoding.encodingmode
    encoding.encoding = "UTF-8"
    encoding.encodingmode = "replace"

    def hgcmd(cmdname, *args, **additional_opts):
        cmdargs = ["hg", *configargs, *cmdname.split(), *args]
        for flagname, flagvalue in additional_opts.items():
            flagname = flagname.replace("_", "-")
            if isinstance(flagvalue, list):
                cmdargs += [f"--{flagname}={v}" for v in flagvalue]
            else:
                cmdargs += [f"--{flagname}={flagvalue}"]
        fin = util.stringio()
        fout = ferr = util.stringio()
        status = bindings.commands.run(cmdargs, fin, fout, ferr)

        output = fout.getvalue().decode()
        if status != 0:
            output += f"[{status}]\n"
        return output

    basic = [
        ("date", lambda: time.ctime()),
        ("unixname", lambda: encoding.environ.get("LOGNAME")),
        ("hostname", lambda: socket.gethostname()),
        ("repo location", lambda: repo.root),
        ("repo svfs location", lambda: repo.svfs.join("")),
        ("cwd", lambda: pycompat.getcwd()),
        ("fstype", lambda: util.getfstype(repo.root)),
        ("active bookmark", lambda: bookmarks._readactive(repo, repo._bookmarks)),
        (
            "hg version",
            lambda: __import__("edenscm.__version__").__version__.version,
        ),
    ]

    def _edenfs_rage():
        ragecmd = "edenfsctl rage --stdout"
        if opts.get("preview"):
            return shcmd(ragecmd + " --dry-run")
        return shcmd(ragecmd)

    detailed = [
        (
            "disk space usage",
            lambda: shcmd(
                "wmic LogicalDisk Where DriveType=3 Get DeviceId,FileSystem,FreeSpace,Size"
                if pycompat.iswindows
                else "df -h",
                check=False,
            ),
        ),
        # smartlog as the user sees it
        ("hg sl", lambda: hgcmd("smartlog", template="{sl_debug}")),
        (
            "hg debugmetalog -t 'since 2d ago'",
            lambda: hgcmd("debugmetalog", time_range=["since 2d ago"]),
        ),
        (
            'first 20 lines of "hg status"',
            lambda: "\n".join(hgcmd("status").splitlines()[:20]),
        ),
        (
            "hg debugmutation -r 'draft() & date(-4)' -t 'since 4d ago'",
            lambda: hgcmd(
                "debugmutation", rev=["draft() & date(-4)"], time_range=["since 4d ago"]
            ),
        ),
        (
            "hg bookmark --list-subscriptions",
            lambda: hgcmd("bookmark", list_subscriptions=True),
        ),
        ("sigtrace", lambda: readsigtraces(repo)),
        (
            "hg blackbox",
            lambda: "\n".join(
                hgcmd("blackbox", pattern=BLACKBOX_PATTERN).splitlines()[-500:]
            ),
        ),
        ("hg summary", lambda: hgcmd("summary")),
        ("hg cloud status", lambda: hgcmd("cloud status")),
        ("hg debugprocesstree", lambda: hgcmd("debugprocesstree")),
        ("hg debugrunlog", lambda: hgcmd("debugrunlog")),
        ("hg config (local)", lambda: "\n".join(localconfig(ui))),
        ("hg sparse", lambda: hgcmd("sparse")),
        ("hg debugchangelog", lambda: hgcmd("debugchangelog")),
        ("hg debugexpandpaths", lambda: hgcmd("debugexpandpaths")),
        ("hg debuginstall", lambda: hgcmd("debuginstall")),
        ("hg debugdetectissues", lambda: hgcmd("debugdetectissues")),
        ("usechg", usechginfo),
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
        ("hg debugnetwork", lambda: hgcmd("debugnetwork")),
        ("hg debugnetworkdoctor", lambda: hgcmd("debugnetworkdoctor")),
        (
            "backedupheads: it is a local cache of what has been backed up",
            lambda: readbackedupheads(repo),
        ),
        ("commit cloud workspace sync state", lambda: readcommitcloudstate(repo)),
        (
            "commitcloud backup logs",
            lambda: infinitepushbackuplogs(ui, repo),
        ),
        ("scm daemon logs", lambda: scmdaemonlog(ui, repo)),
        ("debugstatus", lambda: hgcmd("debugstatus")),
        ("debugtree", lambda: hgcmd("debugtree")),
        ("hg config (all)", lambda: "\n".join(allconfig(ui))),
        ("eden rage", _edenfs_rage),
        (
            "environment variables",
            lambda: "\n".join(
                sorted(["{}={}".format(k, v) for k, v in encoding.environ.items()])
            ),
        ),
        ("ssh config", lambda: shcmd("ssh -G hg.vip.facebook.com", check=False)),
        ("debuglocks", lambda: hgcmd("debuglocks")),
        ("x2pagentd info", lambda: checkproxyagentstate(ui)),
        ("sks-agent rage", lambda: sksagentrage(ui)),
    ]

    msg = ""

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
        with progress.spinner(ui, name):
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

    encoding.encoding = oldencoding
    encoding.encodingmode = oldencodingmode
    return msg


@command("rage", rageopts, _("@prog@ rage"))
def rage(ui, repo, *pats, **opts) -> None:
    """collect troubleshooting diagnostics

    The rage command collects useful diagnostic information.

    By default, the information will be uploaded to Phabricator and
    instructions about how to ask for help will be printed.

    After submitting to Phabricator, it prints configerable advice::

        [rage]
        advice = Please see our FAQ guide: https://...

    """
    with progress.spinner(ui, "collecting"):
        with ui.configoverride({("ui", "color"): "False"}):
            # Disable colors when generating a rage.
            color.setup(ui)
            msg = _makerage(ui, repo, **opts)

    # Restore color output.
    color.setup(ui)

    # Remove all triggered hints.
    hintutil.clear()

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
            out, err = p.communicate(input=pycompat.encodeutf8(msg + "\n"))
            ret = p.returncode
        except OSError:
            ui.write(_("Failed calling pastry. (is it in your PATH?)\n"))
            ret = 1

    if ret:
        fd, tmpname = tempfile.mkstemp(prefix="hg-rage-")
        with util.fdopen(fd, r"w", encoding="utf-8") as tmpfp:
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
        ui.write(
            "  " + pycompat.decodeutf8(out, errors="replace") + "\n", label="rage.link"
        )
    ui.write(ui.config("rage", "advice", "") + "\n")


if pycompat.iswindows:
    colortable = {"rage.link": "yellow bold"}
else:
    colortable = {"rage.link": "blue bold"}
