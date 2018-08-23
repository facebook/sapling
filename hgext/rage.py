# Copyright 2014 Facebook Inc.
#
"""upload useful diagnostics and give instructions for asking for help

    [rage]
    # Name of the rpm binary
    rpmbin = rpm
"""
import datetime
import glob
import json
import os
import re
import socket
import struct
import subprocess
import tempfile
import time
import traceback
from functools import partial

from mercurial import (
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
from mercurial.i18n import _

from . import shareutil
from .remotefilelog import constants, shallowutil


cmdtable = {}
command = registrar.command(cmdtable)


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
    ("p", "preview", None, _("print diagnostic information without doing arc paste"))
]


def localconfig(ui):
    result = []
    for section, name, value in ui.walkconfig():
        source = ui.configsource(section, name)
        if source.find("/etc/") == -1 and source.find("/default.d/") == -1:
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
    logfile = ui.config("commitcloud", "scm_daemon_log_path")

    if not logfile:
        return "commitcloud.scm_daemon_log_path not set"

    try:
        # the user name from the machine
        username = util.getuser()
    except Exception:
        username = "unknown"

    logfile = logfile.replace("%i", username)

    userlogdir = os.path.dirname(logfile)

    if not os.path.exists(userlogdir):
        return "log directory does not exist: %s" % userlogdir

    userlogbasename = os.path.basename(logfile)
    logfiles = [f for f in os.listdir(userlogdir) if userlogbasename in f]

    return _tail(userlogdir, logfiles, 150)


def readinfinitepushbackupstate(repo):
    filename = "infinitepushbackupstate"
    if repo.vfs.exists(filename):
        with repo.vfs.open(filename, "r") as f:
            return json.dumps(json.load(f), indent=4) + "\n"
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


def readfsmonitorstate(repo):
    """
    Read the fsmonitor.state file and pretty print some information from it.
    Based on file format version 4. See hgext/fsmonitor/state.py for real
    implementation.
    """
    f = repo.vfs("fsmonitor.state", "rb")
    versionbytes = f.read(4)
    version = struct.unpack(">I", versionbytes)[0]
    data = f.read()
    state = data.split("\0")
    hostname, clock, ignorehash = state[0:3]
    files = state[3:-1]  # discard empty entry after final file
    numfiles = len(files)
    prettyfiles = "\n".join(files[:20])
    return """\
version: %d
hostname: %s
clock: %s
ignorehash: %s
files (first 20 of %d):
%s
""" % (
        version,
        hostname,
        clock,
        ignorehash,
        numfiles,
        prettyfiles,
    )


def _makerage(ui, repo, **opts):
    srcrepo = shareutil.getsrcrepo(repo)

    def hgcmd(cmdname, *args, **additional_opts):
        cmd, opts = cmdutil.getcmdanddefaultopts(cmdname, commands.table)
        opts.update(additional_opts)

        _repo = repo
        if "_repo" in opts:
            _repo = opts["_repo"]
            del opts["_repo"]
        ui.pushbuffer(error=True)
        try:
            if cmd.norepo:
                cmd(ui, *args, **opts)
            else:
                cmd(ui, _repo, *args, **opts)
        finally:
            return ui.popbuffer()

    def hgsrcrepofile(filename):
        if srcrepo.vfs.exists(filename):
            return srcrepo.vfs(filename).read()
        else:
            return "File not found: %s" % srcrepo.vfs.join(filename)

    basic = [
        ("date", lambda: time.ctime()),
        ("unixname", lambda: encoding.environ.get("LOGNAME")),
        ("hostname", lambda: socket.gethostname()),
        ("repo location", lambda: repo.root),
        ("active bookmark", lambda: bookmarks._readactive(repo, repo._bookmarks)),
        ("hg version", lambda: __import__("mercurial.__version__").__version__.version),
        ("obsstore size", lambda: str(srcrepo.vfs.stat("store/obsstore").st_size)),
    ]

    oldcolormode = ui._colormode
    ui._colormode = None

    detailed = [
        ("df -h", lambda: shcmd("df -h", check=False)),
        ("hg sl (filtered)", lambda: hgcmd("smartlog", template="{sl_debug}")),
        # unfiltered smartlog for recent hidden changesets, including full
        # node identity
        (
            "hg sl (unfiltered)",
            lambda: hgcmd(
                "smartlog", _repo=repo.unfiltered(), template="{node}\n{sl_debug}"
            ),
        ),
        # smartlog as the user sees it
        (
            'first 20 lines of "hg status"',
            lambda: "\n".join(hgcmd("status").splitlines()[:20]),
        ),
        ("hg blackbox -l60", lambda: hgcmd("blackbox", limit=60)),
        ("hg summary", lambda: hgcmd("summary")),
        ("hg debugprocesstree", lambda: hgcmd("debugprocesstree")),
        ("hg config (local)", lambda: "\n".join(localconfig(ui))),
        ("hg sparse", lambda: hgcmd("sparse")),
        ("usechg", (usechginfo)),
        ("uptime", lambda: shcmd("uptime")),
        ("rpm info", (partial(rpminfo, ui))),
        ("klist", lambda: shcmd("klist", check=False)),
        ("ifconfig", lambda: shcmd("ifconfig")),
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
        ("infinitepush backup state", lambda: readinfinitepushbackupstate(srcrepo)),
        ("commit cloud workspace sync state", lambda: readcommitcloudstate(srcrepo)),
        (
            "infinitepush / commitcloud backup logs",
            lambda: infinitepushbackuplogs(ui, repo),
        ),
        ("scm daemon logs", lambda: scmdaemonlog(ui, repo)),
        ("hg config (all)", lambda: hgcmd("config")),
        ("fsmonitor state", lambda: readfsmonitorstate(repo)),
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
                        % (path, shcmd("ls -lhS %s" % path)),
                    )
                )

    # This is quite slow, so we don't want to do it by default
    if ui.configbool("rage", "fastmanifestcached", False):
        detailed.append(
            (
                'hg sl -r "fastmanifestcached()"',
                (lambda: hgcmd("smartlog", rev=["fastmanifestcached()"])),
            )
        )

    footnotes = []

    def _failsafe(gen):
        try:
            return gen()
        except Exception as ex:
            index = len(footnotes) + 1
            footnotes.append(
                "[%d]: %s\n%s\n\n" % (index, str(ex), traceback.format_exc())
            )
            return "(Failed. See footnote [%d])" % index

    msg = []
    profile = []
    allstart = time.time()
    for name, gen in basic:
        msg.append("%s: %s\n\n" % (name, _failsafe(gen)))
    profile.append((time.time() - allstart, "basic info", None))
    for name, gen in detailed:
        start = time.time()
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


@command("^rage", rageopts, _("hg rage"))
def rage(ui, repo, *pats, **opts):
    """collect useful diagnostics for asking help from the source control team

    The rage command collects useful diagnostic information.

    By default, the information will be uploaded to Phabricator and
    instructions about how to ask for help will be printed.

    After submitting to Phabricator, it prints configerable advice::

    [rage]
    advice = Please see our FAQ guide: https://...

    """
    with progress.spinner(ui, "Generating paste"):
        msg = _makerage(ui, repo, **opts)

    if opts.get("preview"):
        ui.write("%s\n" % msg)
        return

    with progress.spinner(ui, "Saving paste"):
        try:
            p = subprocess.Popen(
                ["arc", "paste", "--lang", "hgrage", "--title", "hgrage"],
                stdout=subprocess.PIPE,
                stdin=subprocess.PIPE,
                stderr=subprocess.PIPE,
                shell=pycompat.iswindows,
            )
            out, err = p.communicate(input=msg + "\n")
            ret = p.returncode
        except OSError:
            ui.write(_("Failed calling arc. (is it in your PATH?)\n"))
            ret = 1

    if ret:
        fd, tmpname = tempfile.mkstemp(prefix="hg-rage-")
        with os.fdopen(fd, r"w") as tmpfp:
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
