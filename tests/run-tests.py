#!/usr/bin/env python
#
# run-tests.py - Run a set of tests on Mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Modifying this script is tricky because it has many modes:
#   - serial vs parallel (default) (-jN, N > 1)
#   - no coverage (default) vs coverage (-c, -C, -s)
#   - temp install vs specific hg script (--with-hg, --local (default))
#   - tests are a mix of shell scripts and Python scripts
#
# If you change this script, it is recommended that you ensure you
# haven't broken it by running it in various modes with a representative
# sample of test scripts.  For example:
#
#  1) serial, no coverage, temp install:
#      ./run-tests.py -j1 --build test-s*
#  2) serial, no coverage, local hg:
#      ./run-tests.py -j1 --local test-s*
#  3) serial, coverage, temp install:
#      ./run-tests.py -j1 -b -c test-s*
#  4) serial, coverage, local hg:
#      ./run-tests.py -j1 -c --local test-s*  # unsupported
#  5) parallel, no coverage, temp install:
#      ./run-tests.py -j2 -b test-s*
#  6) parallel, no coverage, local hg:
#      ./run-tests.py -j2 --local test-s*
#  7) parallel, coverage, temp install:
#      ./run-tests.py -j2 -c -b test-s*       # currently broken
#  8) parallel, coverage, local install:
#      ./run-tests.py -j2 -c --local test-s*  # unsupported (and broken)
#  9) parallel, custom tmp dir:
#      ./run-tests.py -j2 --tmpdir /tmp/myhgtests
#  10) parallel, pure, tests that call run-tests:
#      ./run-tests.py --pure `grep -l run-tests.py *.t`
#
# (You could use any subset of the tests: test-s* happens to match
# enough that it's worth doing parallel runs, few enough that it
# completes fairly quickly, includes both shell and Python scripts, and
# includes some scripts that run daemon processes.)

from __future__ import absolute_import, print_function

import argparse
import collections
import difflib
import distutils.version as version
import errno
import hashlib
import json
import multiprocessing
import os
import random
import re
import shutil
import signal
import socket
import subprocess
import sys
import sysconfig
import tempfile
import threading
import time
import unittest
import uuid
import xml.dom.minidom as minidom


try:
    import Queue as queue
except ImportError:
    import queue

try:
    import shlex

    shellquote = shlex.quote
except (ImportError, AttributeError):
    import pipes

    shellquote = pipes.quote

try:
    from edenscmnative.threading import Condition as RLock
except ImportError:
    RLock = threading.RLock

if os.environ.get("RTUNICODEPEDANTRY", False):
    try:
        reload(sys)
        sys.setdefaultencoding("undefined")
    except NameError:
        pass

origenviron = os.environ.copy()
osenvironb = getattr(os, "environb", os.environ)
processlock = threading.Lock()

pygmentspresent = False
# ANSI color is unsupported prior to Windows 10
if os.name != "nt":
    try:  # is pygments installed
        import pygments
        import pygments.lexers as lexers
        import pygments.lexer as lexer
        import pygments.formatters as formatters
        import pygments.token as token
        import pygments.style as style

        pygmentspresent = True
        difflexer = lexers.DiffLexer()
        terminal256formatter = formatters.Terminal256Formatter()
    except ImportError:
        pass

if pygmentspresent:

    class TestRunnerStyle(style.Style):
        default_style = ""
        skipped = token.string_to_tokentype("Token.Generic.Skipped")
        failed = token.string_to_tokentype("Token.Generic.Failed")
        skippedname = token.string_to_tokentype("Token.Generic.SName")
        failedname = token.string_to_tokentype("Token.Generic.FName")
        styles = {
            skipped: "#e5e5e5",
            skippedname: "#00ffff",
            failed: "#7f0000",
            failedname: "#ff0000",
        }

    class TestRunnerLexer(lexer.RegexLexer):
        tokens = {
            "root": [
                (r"^Skipped", token.Generic.Skipped, "skipped"),
                (r"^Failed ", token.Generic.Failed, "failed"),
                (r"^ERROR: ", token.Generic.Failed, "failed"),
            ],
            "skipped": [
                (r"[\w-]+\.(t|py)", token.Generic.SName),
                (r":.*", token.Generic.Skipped),
            ],
            "failed": [
                (r"[\w-]+\.(t|py)", token.Generic.FName),
                (r"(:| ).*", token.Generic.Failed),
            ],
        }

    runnerformatter = formatters.Terminal256Formatter(style=TestRunnerStyle)
    runnerlexer = TestRunnerLexer()

if sys.version_info > (3, 5, 0):
    PYTHON3 = True
    xrange = range  # we use xrange in one place, and we'd rather not use range

    def _bytespath(p):
        if p is None:
            return p
        return p.encode("utf-8")

    def _strpath(p):
        if p is None:
            return p
        return p.decode("utf-8")


elif sys.version_info >= (3, 0, 0):
    print(
        "%s is only supported on Python 3.5+ and 2.7, not %s"
        % (sys.argv[0], ".".join(str(v) for v in sys.version_info[:3]))
    )
    sys.exit(70)  # EX_SOFTWARE from `man 3 sysexit`
else:
    PYTHON3 = False

    # In python 2.x, path operations are generally done using
    # bytestrings by default, so we don't have to do any extra
    # fiddling there. We define the wrapper functions anyway just to
    # help keep code consistent between platforms.
    def _bytespath(p):
        return p

    _strpath = _bytespath

# For Windows support
wifexited = getattr(os, "WIFEXITED", lambda x: False)

# Whether to use IPv6
def checksocketfamily(name, port=20058):
    """return true if we can listen on localhost using family=name

    name should be either 'AF_INET', or 'AF_INET6'.
    port being used is okay - EADDRINUSE is considered as successful.
    """
    family = getattr(socket, name, None)
    if family is None:
        return False
    try:
        s = socket.socket(family, socket.SOCK_STREAM)
        s.bind(("localhost", port))
        s.close()
        return True
    except socket.error as exc:
        if exc.errno == errno.EADDRINUSE:
            return True
        elif exc.errno in (errno.EADDRNOTAVAIL, errno.EPROTONOSUPPORT):
            return False
        else:
            raise
    else:
        return False


# useipv6 will be set by parseargs
useipv6 = None


def checkportisavailable(port):
    """return true if a port seems free to bind on localhost"""
    if useipv6:
        family = socket.AF_INET6
    else:
        family = socket.AF_INET
    try:
        s = socket.socket(family, socket.SOCK_STREAM)
        s.bind(("localhost", port))
        s.close()
        return True
    except socket.error as exc:
        if exc.errno not in (
            errno.EADDRINUSE,
            errno.EADDRNOTAVAIL,
            errno.EPROTONOSUPPORT,
        ):
            raise
    return False


closefds = os.name == "posix"

if os.name == "nt":
    preexec = None
else:
    preexec = lambda: os.setpgid(0, 0)


def Popen4(cmd, wd, timeout, env=None):
    with processlock:
        p = subprocess.Popen(
            cmd,
            shell=True,
            bufsize=-1,
            cwd=wd,
            env=env,
            close_fds=closefds,
            preexec_fn=preexec,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )

    p.fromchild = p.stdout
    p.tochild = p.stdin
    p.childerr = p.stderr

    p.timeout = False
    if timeout:
        track(p)

        def t():
            start = time.time()
            while time.time() - start < timeout and p.returncode is None:
                time.sleep(0.1)
            p.timeout = True
            if p.returncode is None:
                terminate(p)

        threading.Thread(target=t).start()

    return p


PYTHON = _bytespath(sys.executable.replace("\\", "/"))
IMPL_PATH = b"PYTHONPATH"
if "java" in sys.platform:
    IMPL_PATH = b"JYTHONPATH"

defaults = {
    "jobs": ("HGTEST_JOBS", multiprocessing.cpu_count()),
    "timeout": ("HGTEST_TIMEOUT", 360),
    "slowtimeout": ("HGTEST_SLOWTIMEOUT", 1000),
    "port": ("HGTEST_PORT", 20059),
    "shell": ("HGTEST_SHELL", "bash"),
    "maxdifflines": ("HGTEST_MAXDIFFLINES", 30),
}


def canonpath(path):
    return os.path.realpath(os.path.expanduser(path))


def parselistfiles(files, listtype, warn=True):
    entries = dict()
    for filename in files:
        try:
            path = os.path.expanduser(os.path.expandvars(filename))
            f = open(path, "rb")
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            if warn:
                print("warning: no such %s file: %s" % (listtype, filename))
            continue

        for line in f.readlines():
            line = line.split(b"#", 1)[0].strip()
            if line:
                entries[line] = filename

        f.close()
    return entries


def parsettestcases(path):
    """read a .t test file, return a set of test case names

    If path does not exist, return an empty set.
    """
    cases = set()
    try:
        with open(path, "rb") as f:
            for l in f:
                if l.startswith(b"#testcases "):
                    cases.update(l[11:].split())
    except IOError as ex:
        if ex.errno != errno.ENOENT:
            raise
    return cases


def getparser():
    """Obtain the OptionParser used by the CLI."""
    parser = argparse.ArgumentParser(usage="%(prog)s [options] [tests]")

    selection = parser.add_argument_group("Test Selection")
    selection.add_argument(
        "--allow-slow-tests", action="store_true", help="allow extremely slow tests"
    )
    selection.add_argument(
        "--blacklist",
        action="append",
        help="skip tests listed in the specified blacklist file",
    )
    selection.add_argument(
        "--changed",
        help="run tests that are changed in parent rev or working directory",
    )
    selection.add_argument("-k", "--keywords", help="run tests matching keywords")
    selection.add_argument(
        "-r", "--retest", action="store_true", help="retest failed tests"
    )
    selection.add_argument(
        "--test-list", action="append", help="read tests to run from the specified file"
    )
    selection.add_argument(
        "--whitelist",
        action="append",
        help="always run tests listed in the specified whitelist file",
    )
    selection.add_argument("tests", metavar="TESTS", nargs="*", help="Tests to run")

    harness = parser.add_argument_group("Test Harness Behavior")
    harness.add_argument(
        "--bisect-repo",
        metavar="bisect_repo",
        help=("Path of a repo to bisect. Use together with --known-good-rev"),
    )
    harness.add_argument(
        "-d",
        "--debug",
        action="store_true",
        help="debug mode: write output of test scripts to console"
        " rather than capturing and diffing it (disables timeout)",
    )
    harness.add_argument(
        "-f", "--first", action="store_true", help="exit on the first test failure"
    )
    harness.add_argument(
        "-i",
        "--interactive",
        action="store_true",
        help="prompt to accept changed output",
    )
    harness.add_argument(
        "-u", "--update-output", action="store_true", help="update test outputs"
    )
    harness.add_argument(
        "-j",
        "--jobs",
        type=int,
        help="number of jobs to run in parallel"
        " (default: $%s or %d)" % defaults["jobs"],
    )
    harness.add_argument(
        "--keep-tmpdir",
        action="store_true",
        help="keep temporary directory after running tests",
    )
    harness.add_argument(
        "--known-good-rev",
        metavar="known_good_rev",
        help=(
            "Automatically bisect any failures using this "
            "revision as a known-good revision."
        ),
    )
    harness.add_argument(
        "--list-tests", action="store_true", help="list tests instead of running them"
    )
    harness.add_argument("--loop", action="store_true", help="loop tests repeatedly")
    harness.add_argument(
        "--random", action="store_true", help="run tests in random order"
    )
    harness.add_argument(
        "-p",
        "--port",
        type=int,
        help="port on which servers should listen"
        " (default: $%s or %d)" % defaults["port"],
    )
    harness.add_argument(
        "--profile-runner", action="store_true", help="run statprof on run-tests"
    )
    harness.add_argument(
        "-R", "--restart", action="store_true", help="restart at last error"
    )
    harness.add_argument(
        "--runs-per-test",
        type=int,
        dest="runs_per_test",
        help="run each test N times (default=1)",
        default=1,
    )
    harness.add_argument(
        "--shell", help="shell to use (default: $%s or %s)" % defaults["shell"]
    )
    harness.add_argument(
        "--showchannels", action="store_true", help="show scheduling channels"
    )
    harness.add_argument(
        "--noprogress", action="store_true", help="do not show progress"
    )
    harness.add_argument(
        "--slowtimeout",
        type=int,
        help="kill errant slow tests after SLOWTIMEOUT seconds"
        " (default: $%s or %d)" % defaults["slowtimeout"],
    )
    harness.add_argument(
        "-t",
        "--timeout",
        type=int,
        help="kill errant tests after TIMEOUT seconds"
        " (default: $%s or %d)" % defaults["timeout"],
    )
    harness.add_argument(
        "--tmpdir",
        help="run tests in the given temporary directory (implies --keep-tmpdir)",
    )
    harness.add_argument(
        "-v", "--verbose", action="store_true", help="output verbose messages"
    )
    harness.add_argument(
        "--testpilot", action="store_true", help="run tests with testpilot"
    )

    hgconf = parser.add_argument_group("Mercurial Configuration")
    hgconf.add_argument(
        "--chg", action="store_true", help="install and use chg wrapper in place of hg"
    )
    hgconf.add_argument(
        "--watchman", action="store_true", help="shortcut for --with-watchman=watchman"
    )
    hgconf.add_argument("--compiler", help="compiler to build with")
    hgconf.add_argument(
        "--extra-config-opt",
        action="append",
        default=[],
        help="set the given config opt in the test hgrc",
    )
    hgconf.add_argument(
        "--extra-rcpath",
        action="append",
        default=[],
        help="load the given config file or directory in the test hgrc",
    )
    hgconf.add_argument(
        "-l",
        "--local",
        action="store_true",
        help="shortcut for --with-hg=<testdir>/../hg, "
        "and --with-chg=<testdir>/../contrib/chg/chg if --chg is set",
    )
    hgconf.add_argument(
        "-b",
        "--rebuild",
        dest="local",
        action="store_false",
        help="build and install to a temporary location before running tests, "
        "the reverse of --local",
    )
    hgconf.set_defaults(local=True)
    hgconf.add_argument(
        "--ipv6",
        action="store_true",
        help="prefer IPv6 to IPv4 for network related tests",
    )
    hgconf.add_argument(
        "--pure",
        action="store_true",
        help="use pure Python code instead of C extensions",
    )
    hgconf.add_argument(
        "-3",
        "--py3k-warnings",
        action="store_true",
        help="enable Py3k warnings on Python 2.7+",
    )
    hgconf.add_argument(
        "--with-chg", metavar="CHG", help="use specified chg wrapper in place of hg"
    )
    hgconf.add_argument(
        "--with-hg",
        metavar="HG",
        help="test using specified hg script rather than a temporary installation",
    )
    hgconf.add_argument(
        "--with-watchman", metavar="WATCHMAN", help="test using specified watchman"
    )
    # This option should be deleted once test-check-py3-compat.t and other
    # Python 3 tests run with Python 3.
    hgconf.add_argument(
        "--with-python3",
        metavar="PYTHON3",
        help="Python 3 interpreter (if running under Python 2) (TEMPORARY)",
    )

    reporting = parser.add_argument_group("Results Reporting")
    reporting.add_argument(
        "-C",
        "--annotate",
        action="store_true",
        help="output files annotated with coverage",
    )
    reporting.add_argument(
        "--color",
        choices=["always", "auto", "never"],
        default=os.environ.get("HGRUNTESTSCOLOR", "auto"),
        help="colorisation: always|auto|never (default: auto)",
    )
    reporting.add_argument(
        "-c", "--cover", action="store_true", help="print a test coverage report"
    )
    reporting.add_argument(
        "--exceptions",
        action="store_true",
        help="log all exceptions and generate an exception report",
    )
    reporting.add_argument(
        "-H",
        "--htmlcov",
        action="store_true",
        help="create an HTML report of the coverage of the files",
    )
    reporting.add_argument(
        "--json",
        action="store_true",
        help="store test result data in 'report.json' file",
    )
    reporting.add_argument(
        "--outputdir", help="directory to write error logs to (default=test directory)"
    )
    reporting.add_argument(
        "-n", "--nodiff", action="store_true", help="skip showing test changes"
    )
    reporting.add_argument(
        "--maxdifflines",
        type=int,
        help="maximum lines of diff output"
        " (default: $%s or %d)" % defaults["maxdifflines"],
    )
    reporting.add_argument(
        "-S", "--noskips", action="store_true", help="don't report skip tests verbosely"
    )

    reporting.add_argument(
        "--time", action="store_true", help="time how long each test takes"
    )
    reporting.add_argument("--view", help="external diff viewer")
    reporting.add_argument("--xunit", help="record xunit results at specified path")

    for option, (envvar, default) in defaults.items():
        defaults[option] = type(default)(os.environ.get(envvar, default))
    parser.set_defaults(**defaults)

    return parser


def parseargs(args, parser):
    """Parse arguments with our OptionParser and validate results."""
    options = parser.parse_args(args)

    # jython is always pure
    if "java" in sys.platform or "__pypy__" in sys.modules:
        options.pure = True

    if options.with_hg:
        options.with_hg = canonpath(_bytespath(options.with_hg))
        if not (
            os.path.isfile(options.with_hg) and os.access(options.with_hg, os.X_OK)
        ):
            parser.error("--with-hg must specify an executable hg script")
    if options.local:
        testdir = os.path.dirname(_bytespath(canonpath(sys.argv[0])))
        reporootdir = os.path.dirname(testdir)
        pathandattrs = [(b"hg", "with_hg")]
        if options.chg:
            pathandattrs.append((b"contrib/chg/chg", "with_chg"))
        for relpath, attr in pathandattrs:
            if getattr(options, attr, None):
                continue
            binpath = os.path.join(reporootdir, relpath)
            if os.name != "nt" and not os.access(binpath, os.X_OK):
                parser.error(
                    "--local specified, but %r not found or not executable" % binpath
                )
            setattr(options, attr, binpath)

    if (options.chg or options.with_chg) and os.name == "nt":
        parser.error("chg does not work on %s" % os.name)
    if options.with_chg:
        options.chg = False  # no installation to temporary location
        options.with_chg = canonpath(_bytespath(options.with_chg))
        if not (
            os.path.isfile(options.with_chg) and os.access(options.with_chg, os.X_OK)
        ):
            parser.error("--with-chg must specify a chg executable")
    if options.chg and options.with_hg:
        # chg shares installation location with hg
        parser.error(
            "--chg does not work when --with-hg is specified "
            "(use --with-chg instead)"
        )
    if options.watchman and options.with_watchman:
        parser.error(
            "--watchman does not work when --with-watchman is specified "
            "(use --with-watchman instead)"
        )

    if options.color == "always" and not pygmentspresent:
        sys.stderr.write(
            "warning: --color=always ignored because pygments is not installed\n"
        )

    if options.bisect_repo and not options.known_good_rev:
        parser.error("--bisect-repo cannot be used without --known-good-rev")

    global useipv6
    if options.ipv6:
        useipv6 = checksocketfamily("AF_INET6")
    else:
        # only use IPv6 if IPv4 is unavailable and IPv6 is available
        useipv6 = (not checksocketfamily("AF_INET")) and checksocketfamily("AF_INET6")

    options.anycoverage = options.cover or options.annotate or options.htmlcov
    if options.anycoverage:
        try:
            import coverage

            covver = version.StrictVersion(coverage.__version__).version
            if covver < (3, 3):
                parser.error("coverage options require coverage 3.3 or later")
        except ImportError:
            parser.error("coverage options now require the coverage package")

    if options.anycoverage and options.local:
        # this needs some path mangling somewhere, I guess
        parser.error("sorry, coverage options do not work when --local is specified")

    if options.anycoverage and options.with_hg:
        parser.error("sorry, coverage options do not work when --with-hg is specified")

    global verbose
    if options.verbose:
        verbose = ""

    if options.tmpdir:
        options.tmpdir = canonpath(options.tmpdir)

    if options.jobs < 1:
        parser.error("--jobs must be positive")
    if options.update_output:
        options.interactive = True
    if options.interactive and options.debug:
        parser.error("-i/--interactive and -d/--debug are incompatible")
    if options.debug:
        options.noprogress = True
        if options.timeout != defaults["timeout"]:
            sys.stderr.write("warning: --timeout option ignored with --debug\n")
        if options.slowtimeout != defaults["slowtimeout"]:
            sys.stderr.write("warning: --slowtimeout option ignored with --debug\n")
        options.timeout = 0
        options.slowtimeout = 0
    if options.py3k_warnings:
        if PYTHON3:
            parser.error("--py3k-warnings can only be used on Python 2.7")
    if options.with_python3:
        if PYTHON3:
            parser.error("--with-python3 cannot be used when executing with Python 3")

        options.with_python3 = canonpath(options.with_python3)
        # Verify Python3 executable is acceptable.
        proc = subprocess.Popen(
            [options.with_python3, b"--version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        out, _err = proc.communicate()
        ret = proc.wait()
        if ret != 0:
            parser.error("could not determine version of python 3")
        if not out.startswith("Python "):
            parser.error("unexpected output from python3 --version: %s" % out)
        vers = version.LooseVersion(out[len("Python ") :])
        if vers < version.LooseVersion("3.5.0"):
            parser.error(
                "--with-python3 version must be 3.5.0 or greater; got %s" % out
            )

    if options.blacklist:
        options.blacklist = parselistfiles(options.blacklist, "blacklist")
    if options.whitelist:
        options.whitelisted = parselistfiles(options.whitelist, "whitelist")
    else:
        options.whitelisted = {}

    if options.showchannels:
        options.nodiff = True
        options.noprogress = True
    if options.noprogress:
        global showprogress
        showprogress = False

    return options


def rename(src, dst):
    """Like os.rename(), trade atomicity and opened files friendliness
    for existing destination support.
    """
    shutil.copy(src, dst)
    os.remove(src)


_unified_diff = difflib.unified_diff
if PYTHON3:
    import functools

    _unified_diff = functools.partial(difflib.diff_bytes, difflib.unified_diff)


def getdiff(expected, output, ref, err):
    servefail = False
    lines = []
    for line in _unified_diff(
        expected, output, os.path.basename(ref), os.path.basename(err)
    ):
        if line.startswith(b"+++") or line.startswith(b"---"):
            line = line.replace(b"\\", b"/")
            if line.endswith(b" \n"):
                line = line[:-2] + b"\n"
        lines.append(line)
        if not servefail and line.startswith(
            b"+  abort: child process failed to start"
        ):
            servefail = True

    return servefail, lines


verbose = False


def vlog(*msg):
    """Log only when in verbose mode."""
    if verbose is False:
        return

    return log(*msg)


# Bytes that break XML even in a CDATA block: control characters 0-31
# sans \t, \n and \r
CDATA_EVIL = re.compile(br"[\000-\010\013\014\016-\037]")

# Match feature conditionalized output lines in the form, capturing the feature
# list in group 2, and the preceeding line output in group 1:
#
#   output..output (feature !)\n
optline = re.compile(b"(.*) \\((.+?) !\\)\n$")


def cdatasafe(data):
    """Make a string safe to include in a CDATA block.

    Certain control characters are illegal in a CDATA block, and
    there's no way to include a ]]> in a CDATA either. This function
    replaces illegal bytes with ? and adds a space between the ]] so
    that it won't break the CDATA block.
    """
    return CDATA_EVIL.sub(b"?", data).replace(b"]]>", b"] ]>")


def log(*msg):
    """Log something to stdout.

    Arguments are strings to print.
    """
    with iolock:
        if verbose:
            print(verbose, end=" ")
        for m in msg:
            print(m, end=" ")
        print()
        sys.stdout.flush()


def highlightdiff(line, color):
    if not color:
        return line
    assert pygmentspresent
    return pygments.highlight(
        line.decode("latin1"), difflexer, terminal256formatter
    ).encode("latin1")


def highlightmsg(msg, color):
    if not color:
        return msg
    assert pygmentspresent
    return pygments.highlight(msg, runnerlexer, runnerformatter)


_pgroups = {}


def track(proc):
    """Register a process to a process group. So it can be killed later."""
    pgroup = ProcessGroup()
    pid = proc.pid
    pgroup.add(pid)
    _pgroups[pid] = pgroup


def terminate(proc):
    """Terminate subprocess"""
    try:
        pgroup = _pgroups.pop(proc.pid)
        vlog("# Terminating process %d recursively" % proc.pid)
        pgroup.terminate()
    except KeyError:
        vlog("# Terminating process %d" % proc.pid)
        try:
            proc.terminate()
        except OSError:
            pass


def killdaemons(pidfile):
    import killdaemons as killmod

    return killmod.killdaemons(pidfile, tryhard=False, remove=True, logfn=vlog)


if os.name == "nt":

    class ProcessGroup(object):
        """Process group backed by Windows JobObject.

        It provides a clean way to kill processes recursively.
        """

        def __init__(self):
            self._hjob = _kernel32.CreateJobObjectA(None, None)

        def add(self, pid):
            hprocess = _kernel32.OpenProcess(
                PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid
            )
            if not hprocess or hprocess == _INVALID_HANDLE_VALUE:
                raise ctypes.WinError(_kernel32.GetLastError())
            try:
                _kernel32.AssignProcessToJobObject(self._hjob, hprocess)
            finally:
                _kernel32.CloseHandle(hprocess)

        def terminate(self):
            if self._hjob:
                _kernel32.TerminateJobObject(self._hjob, 0)
                _kernel32.CloseHandle(self._hjob)
                self._hjob = 0


else:

    class ProcessGroup(object):
        """Fallback implementation on *nix. Kill process groups.

        This is less reliable than Windows' JobObject, because child processes
        can change their process groups. But it's better than nothing.

        On Linux, the "most correct" solution would be cgroup. But that
        requires root permission.
        """

        def __init__(self):
            self._pids = []

        def add(self, pid):
            self._pids.append(pid)

        def terminate(self):
            for pid in self._pids:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except OSError:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except OSError:
                        pass
            self._pids = []


class Test(unittest.TestCase):
    """Encapsulates a single, runnable test.

    While this class conforms to the unittest.TestCase API, it differs in that
    instances need to be instantiated manually. (Typically, unittest.TestCase
    classes are instantiated automatically by scanning modules.)
    """

    # Status code reserved for skipped tests (used by hghave).
    SKIPPED_STATUS = 80

    def __init__(
        self,
        path,
        outputdir,
        tmpdir,
        keeptmpdir=False,
        debug=False,
        first=False,
        timeout=None,
        startport=None,
        extraconfigopts=None,
        extrarcpaths=None,
        py3kwarnings=False,
        shell=None,
        hgcommand=None,
        slowtimeout=None,
        usechg=False,
        useipv6=False,
        watchman=None,
    ):
        """Create a test from parameters.

        path is the full path to the file defining the test.

        tmpdir is the main temporary directory to use for this test.

        keeptmpdir determines whether to keep the test's temporary directory
        after execution. It defaults to removal (False).

        debug mode will make the test execute verbosely, with unfiltered
        output.

        timeout controls the maximum run time of the test. It is ignored when
        debug is True. See slowtimeout for tests with #require slow.

        slowtimeout overrides timeout if the test has #require slow.

        startport controls the starting port number to use for this test. Each
        test will reserve 3 port numbers for execution. It is the caller's
        responsibility to allocate a non-overlapping port range to Test
        instances.

        extraconfigopts is an iterable of extra hgrc config options. Values
        must have the form "key=value" (something understood by hgrc). Values
        of the form "foo.key=value" will result in "[foo] key=value".

        extrarcpaths is an iterable for extra hgrc paths (files or
        directories).

        py3kwarnings enables Py3k warnings.

        shell is the shell to execute tests in.
        """
        if timeout is None:
            timeout = defaults["timeout"]
        if startport is None:
            startport = defaults["port"]
        if slowtimeout is None:
            slowtimeout = defaults["slowtimeout"]
        self.path = path
        self.bname = os.path.basename(path)
        self.name = _strpath(self.bname)
        self._testdir = os.path.dirname(path)
        self._outputdir = outputdir
        self._tmpname = os.path.basename(path)
        self.errpath = os.path.join(self._outputdir, b"%s.err" % self.bname)

        self._threadtmp = tmpdir
        self._keeptmpdir = keeptmpdir
        self._debug = debug
        self._first = first
        self._timeout = timeout
        self._slowtimeout = slowtimeout
        self._startport = startport
        self._extraconfigopts = extraconfigopts or []
        self._extrarcpaths = extrarcpaths or []
        self._py3kwarnings = py3kwarnings
        self._shell = _bytespath(shell)
        self._hgcommand = hgcommand or b"hg"
        self._usechg = usechg
        self._useipv6 = useipv6
        self._watchman = watchman

        self._aborted = False
        self._daemonpids = []
        self._finished = None
        self._ret = None
        self._out = None
        self._skipped = None
        self._testtmp = None
        self._chgsockdir = None

        self._refout = self.readrefout()

    def readrefout(self):
        """read reference output"""
        # If we're not in --debug mode and reference output file exists,
        # check test output against it.
        if self._debug:
            return None  # to match "out is None"
        elif os.path.exists(self.refpath):
            with open(self.refpath, "rb") as f:
                return f.read().splitlines(True)
        else:
            return []

    # needed to get base class __repr__ running
    @property
    def _testMethodName(self):
        return self.name

    def __str__(self):
        return self.name

    def shortDescription(self):
        return self.name

    def setUp(self):
        """Tasks to perform before run()."""
        self._finished = False
        self._ret = None
        self._out = None
        self._skipped = None

        try:
            os.mkdir(self._threadtmp)
        except OSError as e:
            if e.errno != errno.EEXIST:
                raise

        name = self._tmpname
        self._testtmp = os.path.join(self._threadtmp, name)
        os.mkdir(self._testtmp)

        # Remove any previous output files.
        if os.path.exists(self.errpath):
            try:
                os.remove(self.errpath)
            except OSError as e:
                # We might have raced another test to clean up a .err
                # file, so ignore ENOENT when removing a previous .err
                # file.
                if e.errno != errno.ENOENT:
                    raise

        if self._usechg:
            self._chgsockdir = os.path.join(self._threadtmp, b"%s.chgsock" % name)
            os.mkdir(self._chgsockdir)

        if self._watchman:
            shortname = hashlib.sha1(b"%s" % name).hexdigest()[:6]
            self._watchmandir = os.path.join(
                self._threadtmp, b"%s.watchman" % shortname
            )
            os.mkdir(self._watchmandir)
            cfgfile = os.path.join(self._watchmandir, b"config.json")

            if os.name == "nt":
                sockfile = "\\\\.\\pipe\\watchman-test-%s" % uuid.uuid4().hex
                closefd = False
            else:
                sockfile = os.path.join(self._watchmandir, b"sock")
                closefd = True

            self._watchmansock = sockfile

            clilogfile = os.path.join(self._watchmandir, "cli-log")
            logfile = os.path.join(self._watchmandir, b"log")
            pidfile = os.path.join(self._watchmandir, b"pid")
            statefile = os.path.join(self._watchmandir, b"state")

            with open(cfgfile, "w") as f:
                f.write(json.dumps({}))

            envb = osenvironb.copy()
            envb[b"WATCHMAN_CONFIG_FILE"] = _bytespath(cfgfile)
            envb[b"WATCHMAN_SOCK"] = _bytespath(sockfile)

            argv = [
                self._watchman,
                "--sockname",
                sockfile,
                "--logfile",
                logfile,
                "--pidfile",
                pidfile,
                "--statefile",
                statefile,
                "--foreground",
                "--log-level=2",  # debug logging for watchman
            ]

            with open(clilogfile, "wb") as f:
                self._watchmanproc = subprocess.Popen(
                    argv, env=envb, stdin=None, stdout=f, stderr=f, close_fds=closefd
                )

            # Wait for watchman socket to become available
            argv = [
                self._watchman,
                "--no-spawn",
                "--no-local",
                "--sockname",
                sockfile,
                "version",
            ]
            deadline = time.time() + 30
            watchmanavailable = False
            while not watchmanavailable and time.time() < deadline:
                try:
                    # The watchman CLI can wait for a short time if sockfile
                    # is not ready.
                    subprocess.check_output(argv, env=envb, close_fds=closefd)
                    watchmanavailable = True
                except Exception:
                    time.sleep(0.1)
            if not watchmanavailable:
                # tearDown needs to be manually called in this case.
                self.tearDown()
                raise RuntimeError("timed out waiting for watchman")

    def run(self, result):
        """Run this test and report results against a TestResult instance."""
        # This function is extremely similar to unittest.TestCase.run(). Once
        # we require Python 2.7 (or at least its version of unittest), this
        # function can largely go away.
        self._result = result
        result.startTest(self)
        try:
            try:
                self.setUp()
            except (KeyboardInterrupt, SystemExit):
                self._aborted = True
                raise
            except Exception:
                result.addError(self, sys.exc_info())
                return

            success = False
            try:
                self.runTest()
            except KeyboardInterrupt:
                self._aborted = True
                raise
            except unittest.SkipTest as e:
                result.addSkip(self, str(e))
                # The base class will have already counted this as a
                # test we "ran", but we want to exclude skipped tests
                # from those we count towards those run.
                result.testsRun -= 1
            except self.failureException as e:
                # This differs from unittest in that we don't capture
                # the stack trace. This is for historical reasons and
                # this decision could be revisited in the future,
                # especially for PythonTest instances.
                if result.addFailure(self, str(e)):
                    success = True
            except Exception:
                result.addError(self, sys.exc_info())
            else:
                success = True

            try:
                self.tearDown()
            except (KeyboardInterrupt, SystemExit):
                self._aborted = True
                raise
            except Exception:
                result.addError(self, sys.exc_info())
                success = False

            if success:
                result.addSuccess(self)
        finally:
            result.stopTest(self, interrupted=self._aborted)

    def runTest(self):
        """Run this test instance.

        This will return a tuple describing the result of the test.
        """
        env = self._getenv()
        self._genrestoreenv(env)
        self._daemonpids.append(env["DAEMON_PIDS"])
        self._createhgrc(env["HGRCPATH"].rsplit(os.pathsep, 1)[-1])

        vlog("# Test", self.name)

        ret, out = self._run(env)
        self._finished = True
        self._ret = ret
        self._out = out

        def describe(ret):
            if ret < 0:
                return "killed by signal: %d" % -ret
            return "returned error code %d" % ret

        self._skipped = False

        if ret == self.SKIPPED_STATUS:
            if out is None:  # Debug mode, nothing to parse.
                missing = ["unknown"]
                failed = None
            else:
                missing, failed = TTest.parsehghaveoutput(out)

            if not missing:
                missing = ["skipped"]

            if failed:
                self.fail("hg have failed checking for %s" % failed[-1])
            else:
                self._skipped = True
                raise unittest.SkipTest(missing[-1])
        elif ret == "timeout":
            self.fail("timed out")
        elif ret is False:
            self.fail("no result code from test")
        elif out != self._refout:
            # Diff generation may rely on written .err file.
            if (
                (ret != 0 or out != self._refout)
                and not self._skipped
                and not self._debug
            ):
                with open(self.errpath, "wb") as f:
                    for line in out:
                        f.write(line)

            # The result object handles diff calculation for us.
            with firstlock:
                if self._result.addOutputMismatch(self, ret, out, self._refout):
                    # change was accepted, skip failing
                    return
                if self._first:
                    global firsterror
                    firsterror = True

            if ret:
                msg = "output changed and " + describe(ret)
            else:
                msg = "output changed"

            self.fail(msg)
        elif ret:
            self.fail(describe(ret))

    def tearDown(self):
        """Tasks to perform after run()."""
        for entry in self._daemonpids:
            killdaemons(entry)
        self._daemonpids = []

        if self._keeptmpdir:
            log(
                "\nKeeping testtmp dir: %s\nKeeping threadtmp dir: %s"
                % (self._testtmp.decode("utf-8"), self._threadtmp.decode("utf-8"))
            )
        else:
            shutil.rmtree(self._testtmp, True)
            shutil.rmtree(self._threadtmp, True)

        if self._usechg:
            # chgservers will stop automatically after they find the socket
            # files are deleted
            shutil.rmtree(self._chgsockdir, True)

        if self._watchman:
            try:
                self._watchmanproc.terminate()
                self._watchmanproc.kill()
                if self._keeptmpdir:
                    log(
                        "Keeping watchman dir: %s\n" % self._watchmandir.decode("utf-8")
                    )
                else:
                    shutil.rmtree(self._watchmandir, ignore_errors=True)
            except Exception:
                pass

        if (
            (self._ret != 0 or self._out != self._refout)
            and not self._skipped
            and not self._debug
            and self._out
        ):
            with open(self.errpath, "wb") as f:
                for line in self._out:
                    f.write(line)

        vlog("# Ret was:", self._ret, "(%s)" % self.name)

    def _run(self, env):
        # This should be implemented in child classes to run tests.
        raise unittest.SkipTest("unknown test type")

    def abort(self):
        """Terminate execution of this test."""
        self._aborted = True

    def _portmap(self, i):
        offset = b"" if i == 0 else b"%d" % i
        return (br":%d\b" % (self._startport + i), b":$HGPORT%s" % offset)

    def _getreplacements(self):
        """Obtain a mapping of text replacements to apply to test output.

        Test output needs to be normalized so it can be compared to expected
        output. This function defines how some of that normalization will
        occur.
        """
        r = [
            # This list should be parallel to defineport in _getenv
            self._portmap(0),
            self._portmap(1),
            self._portmap(2),
            (br"([^0-9])%s" % re.escape(self._localip()), br"\1$LOCALIP"),
            (br"\bHG_TXNID=TXN:[a-f0-9]{40}\b", br"HG_TXNID=TXN:$ID$"),
        ]
        r.append((self._escapepath(self._testtmp), b"$TESTTMP"))

        replacementfile = os.path.join(self._testdir, b"common-pattern.py")

        if os.path.exists(replacementfile):
            data = {}
            with open(replacementfile, mode="rb") as source:
                # the intermediate 'compile' step help with debugging
                code = compile(source.read(), replacementfile, "exec")
                exec(code, data)
                r.extend(data.get("substitutions", ()))
        return r

    def _escapepath(self, p):
        if os.name == "nt":
            return br"(?:[/\\]{2,4}\?[/\\]{1,2})?" + b"".join(
                c.isalpha()
                and b"[%s%s]" % (c.lower(), c.upper())
                or c in b"/\\"
                and br"[/\\]{1,2}"
                or c.isdigit()
                and c
                or b"\\" + c
                for c in p
            )
        else:
            return re.escape(p)

    def _localip(self):
        if self._useipv6:
            return b"::1"
        else:
            return b"127.0.0.1"

    def _genrestoreenv(self, testenv):
        """Generate a script that can be used by tests to restore the original
        environment."""
        # Put the restoreenv script inside self._threadtmp
        scriptpath = os.path.join(self._threadtmp, b"restoreenv.sh")
        testenv["HGTEST_RESTOREENV"] = scriptpath

        # Only restore environment variable names that the shell allows
        # us to export.
        name_regex = re.compile("^[a-zA-Z][a-zA-Z0-9_]*$")

        # Do not restore these variables; otherwise tests would fail.
        reqnames = {"PYTHON", "TESTDIR", "TESTTMP"}

        with open(scriptpath, "w") as envf:
            for name, value in origenviron.items():
                if not name_regex.match(name):
                    # Skip environment variables with unusual names not
                    # allowed by most shells.
                    continue
                if name in reqnames:
                    continue
                envf.write("%s=%s\n" % (name, shellquote(value)))

            for name in testenv:
                if name in origenviron or name in reqnames:
                    continue
                envf.write("unset %s\n" % (name,))

    def _getenv(self):
        """Obtain environment variables to use during test execution."""

        def defineport(i):
            offset = "" if i == 0 else "%s" % i
            env["HGPORT%s" % offset] = "%s" % (self._startport + i)

        env = os.environ.copy()
        if os.name != "nt":
            # Now that we *only* load stdlib from python.zip on Windows,
            # there's no userbase
            env["PYTHONUSERBASE"] = sysconfig.get_config_var("userbase")
        env["HGEMITWARNINGS"] = "1"
        env["TESTTMP"] = self._testtmp
        env["HOME"] = self._testtmp
        if not self._usechg:
            env["CHGDISABLE"] = "1"
        # This number should match portneeded in _getport
        for port in xrange(3):
            # This list should be parallel to _portmap in _getreplacements
            defineport(port)
        rcpath = os.path.join(self._threadtmp, b".hgrc")
        rcpaths = self._extrarcpaths + [rcpath]
        env["HGRCPATH"] = os.pathsep.join(rcpaths)
        env["DAEMON_PIDS"] = os.path.join(self._threadtmp, b"daemon.pids")
        env["HGEDITOR"] = '"' + sys.executable + '"' + ' -c "import sys; sys.exit(0)"'
        env["HGMERGE"] = "internal:merge"
        env["HGUSER"] = "test"
        env["HGENCODING"] = "ascii"
        env["HGENCODINGMODE"] = "strict"
        env["HGIPV6"] = str(int(self._useipv6))

        # LOCALIP could be ::1 or 127.0.0.1. Useful for tests that require raw
        # IP addresses.
        env["LOCALIP"] = self._localip()

        # Reset some environment variables to well-known values so that
        # the tests produce repeatable output.
        env["LANG"] = env["LC_ALL"] = env["LANGUAGE"] = "C"
        env["TZ"] = "GMT"
        env["EMAIL"] = "Foo Bar <foo.bar@example.com>"
        env["COLUMNS"] = "80"

        # Claim that 256 colors is not supported.
        env["HGCOLORS"] = "16"

        # Do not be affected by system legacy configs.
        env["HGLEGACY"] = ""

        for k in (
            "HG HGPROF CDPATH GREP_OPTIONS http_proxy no_proxy "
            + "HGPLAIN HGPLAINEXCEPT EDITOR VISUAL PAGER "
            + "NO_PROXY CHGDEBUG HGDETECTRACE"
        ).split():
            if k in env:
                del env[k]

        # unset env related to hooks
        for k in env.keys():
            if k.startswith("HG_"):
                del env[k]

        if self._usechg:
            env["CHGSOCKNAME"] = os.path.join(self._chgsockdir, b"server")

        if self._watchman:
            env["WATCHMAN_SOCK"] = self._watchmansock
            env["HGFSMONITOR_TESTS"] = "1"

        return env

    def _createhgrc(self, path):
        """Create an hgrc file for this test."""
        with open(path, "wb") as hgrc:
            hgrc.write(b"[ui]\n")
            hgrc.write(b"slash = True\n")
            hgrc.write(b"interactive = False\n")
            hgrc.write(b"mergemarkers = detailed\n")
            hgrc.write(b"promptecho = True\n")
            hgrc.write(b"[defaults]\n")
            hgrc.write(b"[devel]\n")
            hgrc.write(b"all-warnings = true\n")
            hgrc.write(b"default-date = 0 0\n")
            hgrc.write(b"[lfs]\n")
            if self._watchman:
                hgrc.write(b"[extensions]\nfsmonitor=\n")
                hgrc.write(b"[fsmonitor]\ndetectrace=1\n")
            hgrc.write(b"[web]\n")
            hgrc.write(b"address = localhost\n")
            hgrc.write(b"ipv6 = %s\n" % str(self._useipv6).encode("ascii"))

            # treemanifest
            hgrc.write(b"[extensions]\n")
            hgrc.write(b"treemanifest=\n")
            hgrc.write(b"[treemanifest]\n")
            hgrc.write(b"flatcompat=True\n")
            hgrc.write(b"[remotefilelog]\n")
            hgrc.write(b"reponame=reponame-default\n")
            hgrc.write(b"cachepath=$TESTTMP/default-hgcache\n")

            hgrc.write(b"[commands]\n")
            hgrc.write(
                b"names = absorb|sf add addremove amend annotate|blame archive backfillmanifestrevlog backfilltree backout backupdelete backupdisable backupenable bisect blackbox bookmarks|bookmark bottom branch bundle cachemanifest cat cherry-pick chistedit clone cloud commit|ci config|showconfig|debugconfig copy|cp crecord diff export files fold|squash foo forget fs fsl fssl gc getavailablebackups githelp|git graft grep heads help hide|strip hint histedit histgrep identify|id import|patch incoming|in init isbackedup journal locate log|history manifest merge metaedit metaedit next odiff outgoing|out parents pasterage paths phase prefetch previous prune pull pullbackup purge|clean push pushbackup rage rebase record recover redo reflog remove|rm rename|move|mv repack reset resolve restack revert rollback root sb sba serve share shelve shortlog show sl smartlog|slog sparse split ssl stash status|st summary|sum svn tag tags tip top unamend unbundle uncommit undo unhide unshare unshelve update|up|checkout|co verify verifyremotefilelog version wgrep\n"
            )

            for opt in self._extraconfigopts:
                section, key = opt.encode("utf-8").split(b".", 1)
                assert b"=" in key, (
                    "extra config opt %s must have an = for assignment" % opt
                )
                hgrc.write(b"[%s]\n%s\n" % (section, key))

    def fail(self, msg):
        # unittest differentiates between errored and failed.
        # Failed is denoted by AssertionError (by default at least).
        raise AssertionError(msg)

    def _runcommand(self, cmd, env, normalizenewlines=False, linecallback=None):
        """Run command in a sub-process, capturing the output (stdout and
        stderr).

        Return a tuple (exitcode, output). output is None in debug mode.
        """
        if self._debug:
            proc = subprocess.Popen(cmd, shell=True, cwd=self._testtmp, env=env)
            ret = proc.wait()
            return (ret, None)

        proc = Popen4(cmd, self._testtmp, self._timeout, env)
        track(proc)

        def cleanup():
            terminate(proc)
            ret = proc.wait()
            if ret == 0:
                ret = signal.SIGTERM << 8
            killdaemons(env["DAEMON_PIDS"])
            return ret

        output = ""
        proc.tochild.close()

        try:
            f = proc.fromchild
            while True:
                line = f.readline()
                # Make the test abort faster if other tests are Ctrl+C-ed.
                # Code path: for test in runtests: test.abort()
                if self._aborted:
                    raise KeyboardInterrupt()
                if linecallback:
                    linecallback(line)
                output += line
                if not line:
                    break

        except KeyboardInterrupt:
            vlog("# Handling keyboard interrupt")
            cleanup()
            raise

        finally:
            proc.fromchild.close()

        ret = proc.wait()
        if wifexited(ret):
            ret = os.WEXITSTATUS(ret)

        if proc.timeout:
            ret = "timeout"

        if ret:
            killdaemons(env["DAEMON_PIDS"])

        for s, r in self._getreplacements():
            output = re.sub(s, r, output)

        if normalizenewlines:
            output = output.replace("\r\n", "\n")

        return ret, output.splitlines(True)


class PythonTest(Test):
    """A Python-based test."""

    @property
    def refpath(self):
        return os.path.join(self._testdir, b"%s.out" % self.bname)

    def _processoutput(self, output):
        if os.path.exists(self.refpath):
            expected = open(self.refpath, "r").readlines()
        else:
            return output

        processed = ["" for i in output]
        i = 0
        while i < len(expected) and i < len(output):
            line = expected[i].strip()

            # by default, processed output is the same as received output
            processed[i] = output[i]
            if line.endswith(" (re)"):
                # pattern, should try to match
                pattern = line[:-5]
                if not pattern.endswith("$"):
                    pattern += "$"
                if re.match(pattern, output[i].strip()):
                    processed[i] = expected[i]
            i = i + 1

        # output is longer than expected, we don't need to process
        # the tail
        while i < len(output):
            processed[i] = output[i]
            i = i + 1

        return processed

    def _run(self, env):
        py3kswitch = self._py3kwarnings and b" -3" or b""
        cmd = b'%s%s "%s"' % (PYTHON, py3kswitch, self.path)
        vlog("# Running", cmd)
        normalizenewlines = os.name == "nt"
        result = self._runcommand(cmd, env, normalizenewlines=normalizenewlines)
        if self._aborted:
            raise KeyboardInterrupt()

        return result[0], self._processoutput(result[1])


# Some glob patterns apply only in some circumstances, so the script
# might want to remove (glob) annotations that otherwise should be
# retained.
checkcodeglobpats = [
    # On Windows it looks like \ doesn't require a (glob), but we know
    # better.
    re.compile(br"^pushing to \$TESTTMP/.*[^)]$"),
    re.compile(br"^moving \S+/.*[^)]$"),
    re.compile(br"^pulling from \$TESTTMP/.*[^)]$"),
    # Not all platforms have 127.0.0.1 as loopback (though most do),
    # so we always glob that too.
    re.compile(br".*\$LOCALIP.*$"),
]

bchr = chr
if PYTHON3:
    bchr = lambda x: bytes([x])


class TTest(Test):
    """A "t test" is a test backed by a .t file."""

    SKIPPED_PREFIX = b"skipped: "
    FAILED_PREFIX = b"hghave check failed: "
    NEEDESCAPE = re.compile(br"[\x00-\x08\x0b-\x1f\x7f-\xff]").search

    ESCAPESUB = re.compile(br"[\x00-\x08\x0b-\x1f\\\x7f-\xff]").sub
    ESCAPEMAP = dict((bchr(i), br"\x%02x" % i) for i in range(256))
    ESCAPEMAP.update({b"\\": b"\\\\", b"\r": br"\r"})

    def __init__(self, path, *args, **kwds):
        # accept an extra "case" parameter
        case = kwds.pop("case", None)
        self._case = case
        self._allcases = parsettestcases(path)
        super(TTest, self).__init__(path, *args, **kwds)
        if case:
            self.name = "%s (case %s)" % (self.name, _strpath(case))
            self.errpath = b"%s.%s.err" % (self.errpath[:-4], case)
            self._tmpname += b"-%s" % case
        self._hghavecache = {}

    @property
    def refpath(self):
        return os.path.join(self._testdir, self.bname)

    def _run(self, env):
        with open(self.path, "rb") as f:
            lines = f.readlines()

        # .t file is both reference output and the test input, keep reference
        # output updated with the the test input. This avoids some race
        # conditions where the reference output does not match the actual test.
        if self._refout is not None:
            self._refout = lines

        salt, saltcount, script, after, expected = self._parsetest(lines)
        self.progress = (0, saltcount)

        # Write out the generated script.
        fname = b"%s.sh" % self._testtmp
        with open(fname, "wb") as f:
            for l in script:
                f.write(l)

        cmd = b'%s "%s"' % (self._shell, fname)
        vlog("# Running", cmd)

        saltseen = [0]

        def linecallback(line):
            if salt in line:
                saltseen[0] += 1
                self.progress = (saltseen[0], saltcount)

        exitcode, output = self._runcommand(cmd, env, linecallback=linecallback)

        if self._aborted:
            raise KeyboardInterrupt()

        # Do not merge output if skipped. Return hghave message instead.
        # Similarly, with --debug, output is None.
        if exitcode == self.SKIPPED_STATUS or output is None:
            return exitcode, output

        return self._processoutput(exitcode, output, salt, after, expected)

    def _hghave(self, reqs):
        # Cache the results of _hghave() checks.
        # In some cases the same _hghave() call can be repeated hundreds of
        # times in a row.  (For instance, if a linematch check with a hghave
        # requirement does not match, the _hghave() call will be repeated for
        # each remaining line in the test output.)
        key = tuple(reqs)
        result = self._hghavecache.get(key)
        if result is None:
            result = self._computehghave(reqs)
            self._hghavecache[key] = result
        return result

    def _computehghave(self, reqs):
        # TODO do something smarter when all other uses of hghave are gone.
        runtestdir = os.path.abspath(os.path.dirname(_bytespath(__file__)))
        tdir = runtestdir.replace(b"\\", b"/")
        proc = Popen4(
            b'%s -c "%s/hghave %s"' % (self._shell, tdir, b" ".join(reqs)),
            self._testtmp,
            0,
            self._getenv(),
        )
        stdout, stderr = proc.communicate()
        ret = proc.wait()
        if wifexited(ret):
            ret = os.WEXITSTATUS(ret)
        if ret == 2:
            print(stdout.decode("utf-8"))
            sys.exit(1)

        if ret != 0:
            return False, stdout

        if b"slow" in reqs:
            self._timeout = self._slowtimeout
        return True, None

    def _iftest(self, args):
        # implements "#if"
        reqs = []
        for arg in args:
            if arg.startswith(b"no-") and arg[3:] in self._allcases:
                if arg[3:] == self._case:
                    return False
            elif arg in self._allcases:
                if arg != self._case:
                    return False
            else:
                reqs.append(arg)
        return self._hghave(reqs)[0]

    def _parsetest(self, lines):
        # We generate a shell script which outputs unique markers to line
        # up script results with our source. These markers include input
        # line number and the last return code.
        salt = b"SALT%d" % time.time()
        saltcount = [0]

        def addsalt(line, inpython):
            saltcount[0] += 1
            if inpython:
                script.append(b"%s %d 0\n" % (salt, line))
            else:
                script.append(b"echo %s %d $?\n" % (salt, line))

        script = []

        # After we run the shell script, we re-unify the script output
        # with non-active parts of the source, with synchronization by our
        # SALT line number markers. The after table contains the non-active
        # components, ordered by line number.
        after = {}

        # Expected shell script output.
        expected = {}

        pos = prepos = -1

        # True or False when in a true or false conditional section
        skipping = None

        # We keep track of whether or not we're in a Python block so we
        # can generate the surrounding doctest magic.
        inpython = False

        if self._debug:
            script.append(b"set -x\n")
        if os.getenv("MSYSTEM"):
            script.append(b'pwd() { builtin pwd -W "$@"; }\n')

        # Source $RUNTESTDIR/tinit.sh for utility functions
        script.append(b'source "$RUNTESTDIR/tinit.sh"\n')

        n = 0
        for n, l in enumerate(lines):
            if not l.endswith(b"\n"):
                l += b"\n"
            if l.startswith(b"#require"):
                lsplit = l.split()
                if len(lsplit) < 2 or lsplit[0] != b"#require":
                    after.setdefault(pos, []).append("  !!! invalid #require\n")
                if not skipping:
                    haveresult, message = self._hghave(lsplit[1:])
                    if not haveresult:
                        script = [b'echo "%s"\nexit 80\n' % message]
                        break
                after.setdefault(pos, []).append(l)
            elif l.startswith(b"#if"):
                lsplit = l.split()
                if len(lsplit) < 2 or lsplit[0] != b"#if":
                    after.setdefault(pos, []).append("  !!! invalid #if\n")
                if skipping is not None:
                    after.setdefault(pos, []).append("  !!! nested #if\n")
                skipping = not self._iftest(lsplit[1:])
                after.setdefault(pos, []).append(l)
            elif l.startswith(b"#else"):
                if skipping is None:
                    after.setdefault(pos, []).append("  !!! missing #if\n")
                skipping = not skipping
                after.setdefault(pos, []).append(l)
            elif l.startswith(b"#endif"):
                if skipping is None:
                    after.setdefault(pos, []).append("  !!! missing #if\n")
                skipping = None
                after.setdefault(pos, []).append(l)
            elif skipping:
                after.setdefault(pos, []).append(l)
            elif l.startswith(b"  >>> "):  # python inlines
                after.setdefault(pos, []).append(l)
                prepos = pos
                pos = n
                if not inpython:
                    # We've just entered a Python block. Add the header.
                    inpython = True
                    addsalt(prepos, False)  # Make sure we report the exit code.
                    if os.name == "nt":
                        script.append(
                            b"%s %s <<EOF\n"
                            % (
                                PYTHON,
                                self._stringescape(
                                    os.path.join(self._testdir, "heredoctest.py")
                                ),
                            )
                        )
                    else:
                        script.append(b"%s -m heredoctest <<EOF\n" % PYTHON)
                addsalt(n, True)
                script.append(l[2:])
            elif l.startswith(b"  ... "):  # python inlines
                after.setdefault(prepos, []).append(l)
                script.append(l[2:])
            elif l.startswith(b"  $ "):  # commands
                if inpython:
                    script.append(b"EOF\n")
                    inpython = False
                after.setdefault(pos, []).append(l)
                prepos = pos
                pos = n
                addsalt(n, False)
                cmd = l[4:].split()
                if len(cmd) == 2 and cmd[0] == b"cd":
                    l = b"  $ cd %s || exit 1\n" % cmd[1]
                script.append(l[4:])
            elif l.startswith(b"  > "):  # continuations
                after.setdefault(prepos, []).append(l)
                script.append(l[4:])
            elif l.startswith(b"  "):  # results
                # Queue up a list of expected results.
                expected.setdefault(pos, []).append(l[2:])
            else:
                if inpython:
                    script.append(b"EOF\n")
                    inpython = False
                # Non-command/result. Queue up for merged output.
                after.setdefault(pos, []).append(l)

        if inpython:
            script.append(b"EOF\n")
        if skipping is not None:
            after.setdefault(pos, []).append("  !!! missing #endif\n")
        addsalt(n + 1, False)

        return salt, saltcount[0], script, after, expected

    def _processoutput(self, exitcode, output, salt, after, expected):
        # Merge the script output back into a unified test.
        warnonly = 1  # 1: not yet; 2: yes; 3: for sure not
        if exitcode != 0:
            warnonly = 3

        pos = -1
        postout = []
        for l in output:
            lout, lcmd = l, None
            if salt in l:
                lout, lcmd = l.split(salt, 1)

            while lout:
                if not lout.endswith(b"\n"):
                    lout += b" (no-eol)\n"

                # Find the expected output at the current position.
                els = [None]
                if expected.get(pos, None):
                    els = expected[pos]

                i = 0
                optional = []
                while i < len(els):
                    el = els[i]

                    r = self.linematch(el, lout)
                    if isinstance(r, str):
                        if r == "-glob":
                            lout = "".join(el.rsplit(" (glob)", 1))
                            r = ""  # Warn only this line.
                        elif r == "retry":
                            postout.append(b"  " + el)
                            els.pop(i)
                            break
                        else:
                            log("\ninfo, unknown linematch result: %r\n" % r)
                            r = False
                    if r:
                        els.pop(i)
                        break
                    if el:
                        if el.endswith(b" (?)\n"):
                            optional.append(i)
                        else:
                            m = optline.match(el)
                            if m:
                                conditions = [c for c in m.group(2).split(b" ")]

                                if not self._iftest(conditions):
                                    optional.append(i)

                    i += 1

                if r:
                    if r == "retry":
                        continue
                    # clean up any optional leftovers
                    for i in optional:
                        postout.append(b"  " + els[i])
                    for i in reversed(optional):
                        del els[i]
                    postout.append(b"  " + el)
                else:
                    if self.NEEDESCAPE(lout):
                        lout = TTest._stringescape(b"%s (esc)\n" % lout.rstrip(b"\n"))
                    postout.append(b"  " + lout)  # Let diff deal with it.
                    if r != "":  # If line failed.
                        warnonly = 3  # for sure not
                    elif warnonly == 1:  # Is "not yet" and line is warn only.
                        warnonly = 2  # Yes do warn.
                break
            else:
                # clean up any optional leftovers
                while expected.get(pos, None):
                    el = expected[pos].pop(0)
                    if el:
                        if not el.endswith(b" (?)\n"):
                            m = optline.match(el)
                            if m:
                                conditions = [c for c in m.group(2).split(b" ")]

                                if self._iftest(conditions):
                                    # Don't append as optional line
                                    continue
                            else:
                                continue
                    postout.append(b"  " + el)

            if lcmd:
                # Add on last return code.
                try:
                    ret = int(lcmd.split()[1])
                except ValueError:
                    ret = 1
                if ret != 0:
                    postout.append(b"  [%d]\n" % ret)
                if pos in after:
                    # Merge in non-active test bits.
                    postout += after.pop(pos)
                pos = int(lcmd.split()[0])

        if pos in after:
            postout += after.pop(pos)

        if warnonly == 2:
            exitcode = False  # Set exitcode to warned.

        return exitcode, postout

    @staticmethod
    def rematch(el, l):
        try:
            el = b"(?:" + el + b")"
            # use \Z to ensure that the regex matches to the end of the string
            if os.name == "nt":
                return re.match(el + br"\r?\n\Z", l)
            return re.match(el + br"\n\Z", l)
        except re.error:
            # el is an invalid regex
            return False

    @staticmethod
    def globmatch(el, l):
        # The only supported special characters are * and ? plus / which also
        # matches \ on windows. Escaping of these characters is supported.
        if el + b"\n" == l:
            if os.altsep:
                # matching on "/" is not needed for this line
                for pat in checkcodeglobpats:
                    if pat.match(el):
                        return True
                return b"-glob"
            return True
        el = el.replace(b"$LOCALIP", b"*")
        # $HGPORT might be changed in test. Do a fuzzy match.
        el = el.replace(b"$HGPORT1", b"*")
        el = el.replace(b"$HGPORT2", b"*")
        el = el.replace(b"$HGPORT", b"*")
        i, n = 0, len(el)
        res = b""
        while i < n:
            c = el[i : i + 1]
            i += 1
            if c == b"\\" and i < n and el[i : i + 1] in b"*?\\/":
                res += el[i - 1 : i + 1]
                i += 1
            elif c == b"*":
                res += b".*"
            elif c == b"?":
                res += b"."
            elif c == b"/" and os.altsep:
                res += b"[/\\\\]"
            else:
                res += re.escape(c)
        return TTest.rematch(res, l)

    def linematch(self, el, l):
        retry = False
        if el == l:  # perfect match (fast)
            return True
        if el:
            if el.endswith(b" (?)\n"):
                retry = "retry"
                el = el[:-5] + b"\n"
            else:
                m = optline.match(el)
                if m:
                    conditions = [c for c in m.group(2).split(b" ")]

                    el = m.group(1) + b"\n"
                    if not self._iftest(conditions):
                        retry = "retry"  # Not required by listed features

            if el.endswith(b" (esc)\n"):
                if PYTHON3:
                    el = el[:-7].decode("unicode_escape") + "\n"
                    el = el.encode("utf-8")
                else:
                    el = el[:-7].decode("string-escape") + "\n"
            if el == l or os.name == "nt" and el[:-1] + b"\r\n" == l:
                return True
            if el.endswith(b" (re)\n"):
                return TTest.rematch(el[:-6], l) or retry
            if el.endswith(b" (glob)\n"):
                # ignore '(glob)' added to l by 'replacements'
                if l.endswith(b" (glob)\n"):
                    l = l[:-8] + b"\n"
                return TTest.globmatch(el[:-8], l) or retry
            if os.altsep:
                _l = l.replace(b"\\", b"/")
                if el == _l or os.name == "nt" and el[:-1] + b"\r\n" == _l:
                    return True
        return retry

    @staticmethod
    def parsehghaveoutput(lines):
        """Parse hghave log lines.

        Return tuple of lists (missing, failed):
          * the missing/unknown features
          * the features for which existence check failed"""
        missing = []
        failed = []
        for line in lines:
            if line.startswith(TTest.SKIPPED_PREFIX):
                line = line.splitlines()[0]
                missing.append(line[len(TTest.SKIPPED_PREFIX) :].decode("utf-8"))
            elif line.startswith(TTest.FAILED_PREFIX):
                line = line.splitlines()[0]
                failed.append(line[len(TTest.FAILED_PREFIX) :].decode("utf-8"))

        return missing, failed

    @staticmethod
    def _escapef(m):
        return TTest.ESCAPEMAP[m.group(0)]

    @staticmethod
    def _stringescape(s):
        return TTest.ESCAPESUB(TTest._escapef, s)


firstlock = RLock()
firsterror = False

_iolock = RLock()


class Progress(object):
    def __init__(self):
        self.lines = []
        self.out = sys.stderr

    def clear(self):
        self.update([])

    def update(self, lines):
        content = ""
        toclear = len(self.lines) - len(lines)
        moveup = len(self.lines) - 1
        if toclear > 0:
            content += "\r\033[K\033[1A" * toclear
            moveup -= toclear
        if moveup > 0:
            content += "\033[%dA" % moveup
        if lines:
            # Disable line wrapping while outputing the progress entries
            content += "\x1b[?7l"
            content += "\n".join("\r\033[K%s" % line.rstrip() for line in lines)
            content += "\x1b[?7h"
        self._write(content)
        self.lines = lines

    def setup(self):
        pass

    def finalize(self):
        pass

    def _write(self, content):
        with _iolock:
            self.out.write(content)
            self.out.flush()


progress = Progress()
showprogress = sys.stderr.isatty()

if os.name == "nt":
    import ctypes

    _HANDLE = ctypes.c_void_p
    _DWORD = ctypes.c_ulong
    _INVALID_HANDLE_VALUE = _HANDLE(-1).value
    _STD_ERROR_HANDLE = _DWORD(-12).value

    _LPVOID = ctypes.c_void_p
    _BOOL = ctypes.c_long
    _UINT = ctypes.c_uint
    _HANDLE = ctypes.c_void_p

    _INVALID_HANDLE_VALUE = _HANDLE(-1).value

    ENABLE_VIRTUAL_TERMINAL_PROCESSING = 0x4

    PROCESS_SET_QUOTA = 0x0100
    PROCESS_TERMINATE = 0x0001

    _kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    _kernel32.CreateJobObjectA.argtypes = [_LPVOID, _LPVOID]
    _kernel32.CreateJobObjectA.restype = _HANDLE

    _kernel32.OpenProcess.argtypes = [_DWORD, _BOOL, _DWORD]
    _kernel32.OpenProcess.restype = _HANDLE

    _kernel32.AssignProcessToJobObject.argtypes = [_HANDLE, _HANDLE]
    _kernel32.AssignProcessToJobObject.restype = _BOOL

    _kernel32.TerminateJobObject.argtypes = [_HANDLE, _UINT]
    _kernel32.TerminateJobObject.restype = _BOOL

    _kernel32.CloseHandle.argtypes = [_HANDLE]
    _kernel32.CloseHandle.restype = _BOOL


if showprogress and os.name == "nt":
    # From mercurial/color.py.
    # Enable virtual terminal mode for the associated console.

    handle = _kernel32.GetStdHandle(_STD_ERROR_HANDLE)  # don't close the handle
    if handle == _INVALID_HANDLE_VALUE:
        showprogress = False
    else:
        mode = _DWORD(0)
        if _kernel32.GetConsoleMode(handle, ctypes.byref(mode)):
            if (mode.value & ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0:
                mode.value |= ENABLE_VIRTUAL_TERMINAL_PROCESSING
                if not _kernel32.SetConsoleMode(handle, mode):
                    showprogress = False


class IOLockWithProgress(object):
    def __enter__(self):
        _iolock.acquire()
        progress.clear()

    def __exit__(self, exc_type, exc_value, traceback):
        _iolock.release()


iolock = IOLockWithProgress()


class TestResult(unittest._TextTestResult):
    """Holds results when executing via unittest."""

    # Don't worry too much about accessing the non-public _TextTestResult.
    # It is relatively common in Python testing tools.
    def __init__(self, options, *args, **kwargs):
        super(TestResult, self).__init__(*args, **kwargs)

        self._options = options

        # unittest.TestResult didn't have skipped until 2.7. We need to
        # polyfill it.
        self.skipped = []

        # We have a custom "ignored" result that isn't present in any Python
        # unittest implementation. It is very similar to skipped. It may make
        # sense to map it into skip some day.
        self.ignored = []

        self.times = []
        self._firststarttime = None
        # Data stored for the benefit of generating xunit reports.
        self.successes = []
        self.faildata = {}

        if options.color == "auto":
            self.color = pygmentspresent and self.stream.isatty()
        elif options.color == "never":
            self.color = False
        else:  # 'always', for testing purposes
            self.color = pygmentspresent

    def addFailure(self, test, reason):
        self.failures.append((test, reason))

        if self._options.first:
            self.stop()
        else:
            if reason == "timed out":
                if not showprogress:
                    with iolock:
                        self.stream.write("t")
            else:
                if not self._options.nodiff:
                    with iolock:
                        self.stream.write("\n")
                        # Exclude the '\n' from highlighting to lex correctly
                        formatted = "ERROR: %s output changed\n" % test
                        self.stream.write(highlightmsg(formatted, self.color))
                if not showprogress:
                    with iolock:
                        self.stream.write("!")

            self.stream.flush()

    def addSuccess(self, test):
        if showprogress and not self.showAll:
            super(unittest._TextTestResult, self).addSuccess(test)
        else:
            with iolock:
                super(TestResult, self).addSuccess(test)
        self.successes.append(test)

    def addError(self, test, err):
        if showprogress and not self.showAll:
            super(unittest._TextTestResult, self).addError(test, err)
        else:
            with iolock:
                super(TestResult, self).addError(test, err)
        if self._options.first:
            self.stop()

    # Polyfill.
    def addSkip(self, test, reason):
        self.skipped.append((test, reason))
        if self.showAll:
            with iolock:
                self.stream.writeln("skipped %s" % reason)
        else:
            if not showprogress:
                with iolock:
                    self.stream.write("s")
                    self.stream.flush()

    def addIgnore(self, test, reason):
        self.ignored.append((test, reason))
        if self.showAll:
            with iolock:
                self.stream.writeln("ignored %s" % reason)
        else:
            if reason not in ("not retesting", "doesn't match keyword"):
                if not showprogress:
                    with iolock:
                        self.stream.write("i")
            else:
                self.testsRun += 1
            self.stream.flush()

    def addOutputMismatch(self, test, ret, got, expected):
        """Record a mismatch in test output for a particular test."""
        if self.shouldStop or firsterror:
            # don't print, some other test case already failed and
            # printed, we're just stale and probably failed due to our
            # temp dir getting cleaned up.
            return

        accepted = False
        lines = []

        with iolock:
            if self._options.nodiff:
                pass
            elif self._options.view:
                v = self._options.view
                if PYTHON3:
                    v = _bytespath(v)
                os.system(b"%s %s %s" % (v, test.refpath, test.errpath))
            else:
                servefail, lines = getdiff(expected, got, test.refpath, test.errpath)
                if servefail:
                    raise test.failureException(
                        "server failed to start (HGPORT=%s)" % test._startport
                    )
                else:
                    self.stream.write("\n")
                    if len(lines) > self._options.maxdifflines:
                        omitted = len(lines) - self._options.maxdifflines
                        lines = lines[: self._options.maxdifflines] + [
                            "... (%d lines omitted. set --maxdifflines to see more) ..."
                            % omitted
                        ]
                    for line in lines:
                        line = highlightdiff(line, self.color)
                        if PYTHON3:
                            self.stream.flush()
                            self.stream.buffer.write(line)
                            self.stream.buffer.flush()
                        else:
                            self.stream.write(line)
                            self.stream.flush()

            # handle interactive prompt without releasing iolock
            if self._options.interactive:
                if test.readrefout() != expected:
                    self.stream.write(
                        "Reference output has changed (run again to prompt changes)"
                    )
                else:
                    self.stream.write("Accept this change? [n]o/yes/all ")
                    if self._options.update_output:
                        isyes = True
                    else:
                        answer = sys.stdin.readline().strip()
                        if answer.lower() in ("a", "all"):
                            self._options.update_output = True
                            isyes = True
                        else:
                            isyes = answer.lower() in ("y", "yes")
                    if isyes:
                        if test.path.endswith(b".t"):
                            rename(test.errpath, test.path)
                        else:
                            rename(test.errpath, "%s.out" % test.path)
                        accepted = True
            if not accepted:
                self.faildata[test.name] = b"".join(lines)

        return accepted

    def startTest(self, test):
        super(TestResult, self).startTest(test)

        # os.times module computes the user time and system time spent by
        # child's processes along with real elapsed time taken by a process.
        # This module has one limitation. It can only work for Linux user
        # and not for Windows.
        test.started = os.times()
        if self._firststarttime is None:  # thread racy but irrelevant
            self._firststarttime = test.started[4]

    def stopTest(self, test, interrupted=False):
        super(TestResult, self).stopTest(test)

        test.stopped = os.times()

        starttime = test.started
        endtime = test.stopped
        origin = self._firststarttime
        self.times.append(
            (
                test.name,
                endtime[2] - starttime[2],  # user space CPU time
                endtime[3] - starttime[3],  # sys  space CPU time
                endtime[4] - starttime[4],  # real time
                starttime[4] - origin,  # start date in run context
                endtime[4] - origin,  # end date in run context
            )
        )

        if interrupted:
            with iolock:
                self.stream.writeln(
                    "INTERRUPTED: %s (after %d seconds)"
                    % (test.name, self.times[-1][3])
                )


class TestSuite(unittest.TestSuite):
    """Custom unittest TestSuite that knows how to execute Mercurial tests."""

    def __init__(
        self,
        testdir,
        jobs=1,
        whitelist=None,
        blacklist=None,
        retest=False,
        keywords=None,
        loop=False,
        runs_per_test=1,
        loadtest=None,
        showchannels=False,
        *args,
        **kwargs
    ):
        """Create a new instance that can run tests with a configuration.

        testdir specifies the directory where tests are executed from. This
        is typically the ``tests`` directory from Mercurial's source
        repository.

        jobs specifies the number of jobs to run concurrently. Each test
        executes on its own thread. Tests actually spawn new processes, so
        state mutation should not be an issue.

        If there is only one job, it will use the main thread.

        whitelist and blacklist denote tests that have been whitelisted and
        blacklisted, respectively. These arguments don't belong in TestSuite.
        Instead, whitelist and blacklist should be handled by the thing that
        populates the TestSuite with tests. They are present to preserve
        backwards compatible behavior which reports skipped tests as part
        of the results.

        retest denotes whether to retest failed tests. This arguably belongs
        outside of TestSuite.

        keywords denotes key words that will be used to filter which tests
        to execute. This arguably belongs outside of TestSuite.

        loop denotes whether to loop over tests forever.
        """
        super(TestSuite, self).__init__(*args, **kwargs)

        self._jobs = jobs
        self._whitelist = whitelist
        self._blacklist = blacklist
        self._retest = retest
        self._keywords = keywords
        self._loop = loop
        self._runs_per_test = runs_per_test
        self._loadtest = loadtest
        self._showchannels = showchannels

    def run(self, result):
        # We have a number of filters that need to be applied. We do this
        # here instead of inside Test because it makes the running logic for
        # Test simpler.
        tests = []
        num_tests = [0]
        for test in self._tests:

            def get():
                num_tests[0] += 1
                if getattr(test, "should_reload", False):
                    return self._loadtest(test, num_tests[0])
                return test

            if not os.path.exists(test.path):
                result.addSkip(test, "Doesn't exist")
                continue

            if not (self._whitelist and test.bname in self._whitelist):
                if self._blacklist and test.bname in self._blacklist:
                    result.addSkip(test, "blacklisted")
                    continue

                if self._retest and not os.path.exists(test.errpath):
                    result.addIgnore(test, "not retesting")
                    continue

                if self._keywords:
                    with open(test.path, "rb") as f:
                        t = f.read().lower() + test.bname.lower()
                    ignored = False
                    for k in self._keywords.lower().split():
                        if k not in t:
                            result.addIgnore(test, "doesn't match keyword")
                            ignored = True
                            break

                    if ignored:
                        continue
            for _ in xrange(self._runs_per_test):
                tests.append(get())

        runtests = list(tests)
        done = queue.Queue()
        running = 0

        channels = [""] * self._jobs
        runningtests = collections.OrderedDict()  # {test name: (test, start time)}

        def job(test, result):
            for n, v in enumerate(channels):
                if not v:
                    channel = n
                    break
            else:
                raise ValueError("Could not find output channel")
            runningtests[test.name] = (test, time.time())
            channels[channel] = "=" + test.name[5:].split(".")[0]
            try:
                test(result)
                done.put(None)
            except KeyboardInterrupt:
                pass
            except:  # re-raises
                done.put(("!", test, "run-test raised an error, see traceback"))
                raise
            finally:
                del runningtests[test.name]
                try:
                    channels[channel] = ""
                except IndexError:
                    pass

        def stat():
            count = 0
            while channels:
                d = "\n%03s  " % count
                for n, v in enumerate(channels):
                    if v:
                        d += v[0]
                        channels[n] = v[1:] or "."
                    else:
                        d += " "
                    d += " "
                with iolock:
                    sys.stdout.write(d + "  ")
                    sys.stdout.flush()
                for x in xrange(10):
                    if channels:
                        time.sleep(0.1)
                count += 1

        def singleprogressbar(value, total, char="="):
            if total:
                if value > total:
                    value = total
                progresschars = char * int(value * 20 / total)
                if progresschars and len(progresschars) < 20:
                    progresschars += ">"
                return "[%-20s]" % progresschars
            else:
                return " " * 22

        blacklisted = len(result.skipped)
        initialtestsrun = result.testsRun

        def progressrenderer():
            lines = []
            suitestart = time.time()
            total = len(runtests)
            while channels:
                failed = len(result.failures) + len(result.errors)
                skipped = len(result.skipped) - blacklisted
                testsrun = result.testsRun - initialtestsrun
                remaining = total - testsrun - skipped
                passed = testsrun - failed - len(runningtests)
                now = time.time()
                timepassed = now - suitestart
                lines = []
                runningfrac = 0.0
                for name, (test, teststart) in runningtests.iteritems():
                    try:
                        saltseen, saltcount = getattr(test, "progress")
                        runningfrac += saltseen * 1.0 / saltcount
                        testprogress = singleprogressbar(saltseen, saltcount, char="-")
                    except Exception:
                        testprogress = singleprogressbar(0, 0)
                    lines.append(
                        "%s %-52s %.1fs" % (testprogress, name[:52], now - teststart)
                    )
                lines[0:0] = [
                    "%s %-52s %.1fs"
                    % (
                        singleprogressbar(
                            runningfrac + failed + passed + skipped, total
                        ),
                        "%s Passed. %s Failed. %s Skipped. %s Remaining"
                        % (passed, failed, skipped, remaining),
                        timepassed,
                    )
                ]
                progress.update(lines)
                time.sleep(0.1)

        stoppedearly = False

        if self._showchannels:
            statthread = threading.Thread(target=stat, name="stat")
            statthread.start()
        elif showprogress:
            progressthread = threading.Thread(target=progressrenderer, name="progress")
            progressthread.start()

        try:
            while tests or running:
                if not done.empty() or running == self._jobs or not tests:
                    try:
                        done.get(True, 1)
                        running -= 1
                        if result and result.shouldStop:
                            stoppedearly = True
                            break
                    except queue.Empty:
                        continue
                if tests and not running == self._jobs:
                    test = tests.pop(0)
                    if self._loop:
                        if getattr(test, "should_reload", False):
                            num_tests[0] += 1
                            tests.append(self._loadtest(test, num_tests[0]))
                        else:
                            tests.append(test)
                    if self._jobs == 1:
                        job(test, result)
                    else:
                        t = threading.Thread(
                            target=job, name=test.name, args=(test, result)
                        )
                        t.start()
                    running += 1

            # If we stop early we still need to wait on started tests to
            # finish. Otherwise, there is a race between the test completing
            # and the test's cleanup code running. This could result in the
            # test reporting incorrect.
            if stoppedearly:
                while running:
                    try:
                        done.get(True, 1)
                        running -= 1
                    except queue.Empty:
                        continue
        except KeyboardInterrupt:
            for test in runtests:
                test.abort()

        channels = []

        return result


# Save the most recent 5 wall-clock runtimes of each test to a
# human-readable text file named .testtimes. Tests are sorted
# alphabetically, while times for each test are listed from oldest to
# newest.


def loadtimes(outputdir):
    times = []
    try:
        with open(os.path.join(outputdir, b".testtimes-")) as fp:
            for line in fp:
                ts = line.split()
                times.append((ts[0], [float(t) for t in ts[1:]]))
    except IOError as err:
        if err.errno != errno.ENOENT:
            raise
    return times


def savetimes(outputdir, result):
    saved = dict(loadtimes(outputdir))
    maxruns = 5
    skipped = set([str(t[0]) for t in result.skipped])
    for tdata in result.times:
        test, real = tdata[0], tdata[3]
        if test not in skipped:
            ts = saved.setdefault(test, [])
            ts.append(real)
            ts[:] = ts[-maxruns:]

    fd, tmpname = tempfile.mkstemp(prefix=b".testtimes", dir=outputdir, text=True)
    with os.fdopen(fd, "w") as fp:
        for name, ts in sorted(saved.items()):
            fp.write("%s %s\n" % (name, " ".join(["%.3f" % (t,) for t in ts])))
    timepath = os.path.join(outputdir, b".testtimes")
    try:
        os.unlink(timepath)
    except OSError:
        pass
    try:
        os.rename(tmpname, timepath)
    except OSError:
        pass


def xunit_time(t):
    # The TestPilot documentation says to follow the JUnit spec, which says time
    # should be in seconds, but that's not quite what TestPilot actually does
    # (see D2822375: it expects the time in milliseconds). There's other code at
    # Facebook that relies on the TESTPILOT_PROCESS to identify if we're running
    # tests, so it should be reasonably safe (albeit hacky) to rely on this.
    if os.environ.get("TESTPILOT_PROCESS"):
        t = t * 1000
    return "%.3f" % t


class TextTestRunner(unittest.TextTestRunner):
    """Custom unittest test runner that uses appropriate settings."""

    def __init__(self, runner, *args, **kwargs):
        super(TextTestRunner, self).__init__(*args, **kwargs)

        self._runner = runner

    def listtests(self, test):
        result = TestResult(self._runner.options, self.stream, self.descriptions, 0)
        test = sorted(test, key=lambda t: t.name)
        for t in test:
            print(t.name)
            result.addSuccess(t)

        if self._runner.options.xunit:
            with open(self._runner.options.xunit, "wb") as xuf:
                self._writexunit(result, xuf)

        if self._runner.options.json:
            jsonpath = os.path.join(self._runner._outputdir, b"report.json")
            with open(jsonpath, "w") as fp:
                self._writejson(result, fp)

        return result

    def run(self, test):
        result = TestResult(
            self._runner.options, self.stream, self.descriptions, self.verbosity
        )

        test(result)

        failed = len(result.failures)
        skipped = len(result.skipped)
        ignored = len(result.ignored)

        with iolock:
            self.stream.writeln("")

            if not self._runner.options.noskips:
                for test, msg in result.skipped:
                    formatted = "Skipped %s: %s\n" % (test.name, msg)
                    self.stream.write(highlightmsg(formatted, result.color))
            for test, msg in result.failures:
                formatted = "Failed %s: %s\n" % (test.name, msg)
                self.stream.write(highlightmsg(formatted, result.color))
            for test, msg in result.errors:
                self.stream.writeln("Errored %s: %s" % (test.name, msg))

            if self._runner.options.xunit:
                with open(self._runner.options.xunit, "wb") as xuf:
                    self._writexunit(result, xuf)

            if self._runner.options.json:
                jsonpath = os.path.join(self._runner._outputdir, b"report.json")
                with open(jsonpath, "w") as fp:
                    self._writejson(result, fp)

            self._runner._checkhglib("Tested")

            savetimes(self._runner._outputdir, result)

            if failed and self._runner.options.known_good_rev:
                self._bisecttests(t for t, m in result.failures)
            self.stream.writeln(
                "# Ran %d tests, %d skipped, %d failed."
                % (result.testsRun, skipped + ignored, failed)
            )
            result.testsSkipped = skipped + ignored
            if failed:
                self.stream.writeln(
                    "python hash seed: %s" % os.environ["PYTHONHASHSEED"]
                )
            if self._runner.options.time:
                self.printtimes(result.times)

            if self._runner.options.exceptions:
                exceptions = aggregateexceptions(
                    os.path.join(self._runner._outputdir, b"exceptions")
                )
                total = sum(exceptions.values())

                self.stream.writeln("Exceptions Report:")
                self.stream.writeln(
                    "%d total from %d frames" % (total, len(exceptions))
                )
                for (frame, line, exc), count in exceptions.most_common():
                    self.stream.writeln("%d\t%s: %s" % (count, frame, exc))

            self.stream.flush()

        return result

    def _bisecttests(self, tests):
        bisectcmd = ["hg", "bisect"]
        bisectrepo = self._runner.options.bisect_repo
        if bisectrepo:
            bisectcmd.extend(["-R", os.path.abspath(bisectrepo)])

        def pread(args):
            env = os.environ.copy()
            env["HGPLAIN"] = "1"
            p = subprocess.Popen(
                args, stderr=subprocess.STDOUT, stdout=subprocess.PIPE, env=env
            )
            data = p.stdout.read()
            p.wait()
            return data

        for test in tests:
            pread(bisectcmd + ["--reset"]),
            pread(bisectcmd + ["--bad", "."])
            pread(bisectcmd + ["--good", self._runner.options.known_good_rev])
            # TODO: we probably need to forward more options
            # that alter hg's behavior inside the tests.
            opts = ""
            withhg = self._runner.options.with_hg
            if withhg:
                opts += " --with-hg=%s " % shellquote(_strpath(withhg))
            rtc = "%s %s %s %s" % (sys.executable, sys.argv[0], opts, test)
            data = pread(bisectcmd + ["--command", rtc])
            m = re.search(
                (
                    br"\nThe first (?P<goodbad>bad|good) revision "
                    br"is:\nchangeset: +\d+:(?P<node>[a-f0-9]+)\n.*\n"
                    br"summary: +(?P<summary>[^\n]+)\n"
                ),
                data,
                (re.MULTILINE | re.DOTALL),
            )
            if m is None:
                self.stream.writeln("Failed to identify failure point for %s" % test)
                continue
            dat = m.groupdict()
            verb = "broken" if dat["goodbad"] == "bad" else "fixed"
            self.stream.writeln(
                "%s %s by %s (%s)" % (test, verb, dat["node"], dat["summary"])
            )

    def printtimes(self, times):
        # iolock held by run
        self.stream.writeln("# Producing time report")
        times.sort(key=lambda t: (t[3]))
        cols = "%7.3f %7.3f %7.3f %7.3f %7.3f   %s"
        self.stream.writeln(
            "%-7s %-7s %-7s %-7s %-7s   %s"
            % ("start", "end", "cuser", "csys", "real", "Test")
        )
        for tdata in times:
            test = tdata[0]
            cuser, csys, real, start, end = tdata[1:6]
            self.stream.writeln(cols % (start, end, cuser, csys, real, test))

    @staticmethod
    def _writexunit(result, outf):
        # See http://llg.cubic.org/docs/junit/ for a reference.
        timesd = dict((t[0], t[3]) for t in result.times)
        doc = minidom.Document()
        s = doc.createElement("testsuite")
        s.setAttribute("name", "run-tests")
        s.setAttribute("tests", str(result.testsRun))
        s.setAttribute("errors", "0")  # TODO
        s.setAttribute("failures", str(len(result.failures)))
        s.setAttribute("skipped", str(len(result.skipped) + len(result.ignored)))
        doc.appendChild(s)
        for tc in result.successes:
            t = doc.createElement("testcase")
            t.setAttribute("name", tc.name)
            tctime = timesd.get(tc.name)
            if tctime is not None:
                t.setAttribute("time", xunit_time(tctime))
            s.appendChild(t)
        for tc, err in sorted(result.faildata.items()):
            t = doc.createElement("testcase")
            t.setAttribute("name", tc)
            tctime = timesd.get(tc)
            if tctime is not None:
                t.setAttribute("time", xunit_time(tctime))
            # createCDATASection expects a unicode or it will
            # convert using default conversion rules, which will
            # fail if string isn't ASCII.
            err = cdatasafe(err).decode("utf-8", "replace")
            cd = doc.createCDATASection(err)
            # Use 'failure' here instead of 'error' to match errors = 0,
            # failures = len(result.failures) in the testsuite element.
            failelem = doc.createElement("failure")
            failelem.setAttribute("message", "output changed")
            failelem.setAttribute("type", "output-mismatch")
            failelem.appendChild(cd)
            t.appendChild(failelem)
            s.appendChild(t)
        for tc, message in result.skipped:
            # According to the schema, 'skipped' has no attributes. So store
            # the skip message as a text node instead.
            t = doc.createElement("testcase")
            t.setAttribute("name", tc.name)
            binmessage = message.encode("utf-8")
            message = cdatasafe(binmessage).decode("utf-8", "replace")
            cd = doc.createCDATASection(message)
            skipelem = doc.createElement("skipped")
            skipelem.appendChild(cd)
            t.appendChild(skipelem)
            s.appendChild(t)
        outf.write(doc.toprettyxml(indent="  ", encoding="utf-8"))

    @staticmethod
    def _writejson(result, outf):
        timesd = {}
        for tdata in result.times:
            test = tdata[0]
            timesd[test] = tdata[1:]

        outcome = {}
        groups = [
            ("success", ((tc, None) for tc in result.successes)),
            ("failure", result.failures),
            ("skip", result.skipped),
        ]
        for res, testcases in groups:
            for tc, __ in testcases:
                if tc.name in timesd:
                    diff = result.faildata.get(tc.name, b"")
                    try:
                        diff = diff.decode("unicode_escape")
                    except UnicodeDecodeError as e:
                        diff = "%r decoding diff, sorry" % e
                    tres = {
                        "result": res,
                        "time": ("%0.3f" % timesd[tc.name][2]),
                        "cuser": ("%0.3f" % timesd[tc.name][0]),
                        "csys": ("%0.3f" % timesd[tc.name][1]),
                        "start": ("%0.3f" % timesd[tc.name][3]),
                        "end": ("%0.3f" % timesd[tc.name][4]),
                        "diff": diff,
                    }
                else:
                    # blacklisted test
                    tres = {"result": res}

                outcome[tc.name] = tres
        outf.write(
            json.dumps(outcome, sort_keys=True, indent=4, separators=(",", ": "))
        )


class TestpilotTestResult(object):
    def __init__(self, testpilotjson):
        self.testsSkipped = 0
        self.errors = 0
        self.failures = []

        with open(testpilotjson, "r") as fp:
            for line in fp:
                result = json.loads(line)
                summary = result["summary"]
                if summary != "passed":
                    if summary == "OMITTED" or summary == "skipped":
                        self.testsSkipped += 1
                    else:
                        self.failures += result
                        self.errors += 1


class TestpilotTestRunner(object):
    def __init__(self, runner):
        self._runner = runner

    def run(self, tests):
        testlist = []

        for t in tests:
            testlist.append(
                {
                    "type": "custom",
                    "target": "hg-%s-%s" % (sys.platform, t),
                    "command": [os.path.abspath(_bytespath(__file__)), "%s" % t.path],
                }
            )

        jsonpath = os.path.join(self._runner._outputdir, b"buck-test-info.json")
        with open(jsonpath, "w") as fp:
            json.dump(testlist, fp)

        # Remove the proxy settings. If they are set, testpilot will hang
        # trying to get test information from the network
        env = os.environ
        env["http_proxy"] = ""
        env["https_proxy"] = ""

        testpilotjson = os.path.join(self._runner._outputdir, b"testpilot-result.json")
        try:
            os.remove(testpilotjson)
        except OSError as e:
            if e.errno != errno.ENOENT:
                raise

        subprocess.call(
            [
                "testpilot",
                "--buck-test-info",
                jsonpath,
                "--print-json-to-file",
                testpilotjson,
            ],
            env=env,
        )

        return TestpilotTestResult(testpilotjson)


class TestRunner(object):
    """Holds context for executing tests.

    Tests rely on a lot of state. This object holds it for them.
    """

    # Programs required to run tests.
    REQUIREDTOOLS = [
        b"diff",
        b"grep",
        b"unzip",
        b"gunzip",
        b"bunzip2",
        b"sed",
        b"cmp",
        b"dd",
    ]

    # Maps file extensions to test class.
    TESTTYPES = [(b".py", PythonTest), (b".t", TTest)]

    def __init__(self):
        self.options = None
        self._hgroot = None
        self._testdir = None
        self._outputdir = None
        self._hgtmp = None
        self._installdir = None
        self._bindir = None
        self._tmpbinddir = None
        self._pythondir = None
        self._coveragefile = None
        self._createdfiles = []
        self._hgcommand = None
        self._hgpath = None
        self._portoffset = 0
        self._ports = {}

    def run(self, args, parser=None):
        """Run the test suite."""
        oldmask = os.umask(0o22)
        try:
            if showprogress:
                progress.setup()
            parser = parser or getparser()
            options = parseargs(args, parser)
            tests = [_bytespath(a) for a in options.tests]
            if options.test_list is not None:
                for listfile in options.test_list:
                    with open(listfile, "rb") as f:
                        tests.extend(t for t in f.read().splitlines() if t)
            self.options = options

            self._checktools()
            testdescs = self.findtests(tests)
            if options.profile_runner:
                import statprof

                statprof.start()
            result = self._run(testdescs)
            if options.profile_runner:
                statprof.stop()
                statprof.display()
            return result

        finally:
            if showprogress:
                progress.finalize()
            os.umask(oldmask)

    def _run(self, testdescs):
        if self.options.random:
            random.shuffle(testdescs)
        else:
            # keywords for slow tests
            slow = {
                b"svn": 10,
                b"cvs": 10,
                b"hghave": 10,
                b"run-tests": 10,
                b"corruption": 10,
                b"race": 10,
                b"i18n": 10,
                b"check": 100,
                b"gendoc": 100,
                b"contrib-perf": 200,
            }
            perf = {}

            def sortkey(f):
                # run largest tests first, as they tend to take the longest
                f = f["path"]
                try:
                    return perf[f]
                except KeyError:
                    try:
                        val = -os.stat(f).st_size
                    except OSError as e:
                        if e.errno != errno.ENOENT:
                            raise
                        perf[f] = -1e9  # file does not exist, tell early
                        return -1e9
                    for kw, mul in slow.items():
                        if kw in f:
                            val *= mul
                    if f.endswith(b".py"):
                        val /= 10.0
                    perf[f] = val / 1000.0
                    return perf[f]

            testdescs.sort(key=sortkey)

        self._testdir = osenvironb[b"TESTDIR"] = getattr(os, "getcwdb", os.getcwd)()
        # assume all tests in same folder for now
        if testdescs:
            pathname = os.path.dirname(testdescs[0]["path"])
            if pathname:
                osenvironb[b"TESTDIR"] = os.path.join(osenvironb[b"TESTDIR"], pathname)
        if self.options.outputdir:
            self._outputdir = canonpath(_bytespath(self.options.outputdir))
        else:
            self._outputdir = self._testdir
            if testdescs and pathname:
                self._outputdir = os.path.join(self._outputdir, pathname)

        if "PYTHONHASHSEED" not in os.environ:
            # use a random python hash seed all the time
            # we do the randomness ourself to know what seed is used
            os.environ["PYTHONHASHSEED"] = str(random.getrandbits(32))

        if self.options.tmpdir:
            self.options.keep_tmpdir = True
            tmpdir = _bytespath(self.options.tmpdir)
            if os.path.exists(tmpdir):
                # Meaning of tmpdir has changed since 1.3: we used to create
                # HGTMP inside tmpdir; now HGTMP is tmpdir.  So fail if
                # tmpdir already exists.
                print("error: temp dir %r already exists" % tmpdir)
                return 1

                # Automatically removing tmpdir sounds convenient, but could
                # really annoy anyone in the habit of using "--tmpdir=/tmp"
                # or "--tmpdir=$HOME".
                # vlog("# Removing temp dir", tmpdir)
                # shutil.rmtree(tmpdir)
            os.makedirs(tmpdir)
        else:
            d = None
            if os.name == "nt":
                # without this, we get the default temp dir location, but
                # in all lowercase, which causes troubles with paths (issue3490)
                d = osenvironb.get(b"TMP", None)
            tmpdir = tempfile.mkdtemp(b"", b"hgtests.", d)

        self._hgtmp = osenvironb[b"HGTMP"] = os.path.realpath(tmpdir)

        if self.options.with_hg:
            self._installdir = None
            whg = self.options.with_hg
            self._bindir = os.path.dirname(os.path.realpath(whg))
            assert isinstance(self._bindir, bytes)
            # use full path, since _hgcommand will also be used as ui.remotecmd
            self._hgcommand = os.path.realpath(whg)
            self._tmpbindir = os.path.join(self._hgtmp, b"install", b"bin")
            os.makedirs(self._tmpbindir)

            # This looks redundant with how Python initializes sys.path from
            # the location of the script being executed.  Needed because the
            # "hg" specified by --with-hg is not the only Python script
            # executed in the test suite that needs to import 'mercurial'
            # ... which means it's not really redundant at all.
            self._pythondir = self._bindir
        else:
            self._installdir = os.path.join(self._hgtmp, b"install")
            self._bindir = os.path.join(self._installdir, b"bin")
            self._hgcommand = os.path.join(self._bindir, b"hg")
            self._tmpbindir = self._bindir
            self._pythondir = os.path.join(self._installdir, b"lib", b"python")

        # set CHGHG, then replace "hg" command by "chg"
        chgbindir = self._bindir
        if self.options.chg or self.options.with_chg:
            osenvironb[b"CHGHG"] = os.path.join(self._bindir, self._hgcommand)
        else:
            osenvironb.pop(b"CHGHG", None)  # drop flag for hghave
        if self.options.chg:
            self._hgcommand = b"chg"
        elif self.options.with_chg:
            chgbindir = os.path.dirname(os.path.realpath(self.options.with_chg))
            self._hgcommand = os.path.basename(self.options.with_chg)
        if self.options.with_watchman or self.options.watchman:
            self._watchman = self.options.with_watchman or "watchman"
            osenvironb[b"HGFSMONITOR_TESTS"] = b"1"
        else:
            osenvironb[b"BINDIR"] = self._bindir
            self._watchman = None
            if b"HGFSMONITOR_TESTS" in osenvironb:
                del osenvironb[b"HGFSMONITOR_TESTS"]

        osenvironb[b"BINDIR"] = self._bindir
        osenvironb[b"PYTHON"] = PYTHON

        if self.options.with_python3:
            osenvironb[b"PYTHON3"] = self.options.with_python3

        fileb = _bytespath(__file__)
        runtestdir = os.path.abspath(os.path.dirname(fileb))
        osenvironb[b"RUNTESTDIR"] = runtestdir
        if PYTHON3:
            sepb = _bytespath(os.pathsep)
        else:
            sepb = os.pathsep
        path = [self._bindir, runtestdir] + osenvironb[b"PATH"].split(sepb)
        if os.path.islink(__file__):
            # test helper will likely be at the end of the symlink
            realfile = os.path.realpath(fileb)
            realdir = os.path.abspath(os.path.dirname(realfile))
            path.insert(2, realdir)
        if chgbindir != self._bindir:
            path.insert(1, chgbindir)
        if self._testdir != runtestdir:
            path = [self._testdir] + path
        if self._tmpbindir != self._bindir:
            path = [self._tmpbindir] + path
        osenvironb[b"PATH"] = sepb.join(path)

        # Include TESTDIR in PYTHONPATH so that out-of-tree extensions
        # can run .../tests/run-tests.py test-foo where test-foo
        # adds an extension to HGRC. Also include run-test.py directory to
        # import modules like heredoctest.
        # self._pythondir should make "import mercurial" do the right thing.
        pypath = [self._pythondir, self._testdir, runtestdir]
        # We have to augment PYTHONPATH, rather than simply replacing
        # it, in case external libraries are only available via current
        # PYTHONPATH.  (In particular, the Subversion bindings on OS X
        # are in /opt/subversion.)
        oldpypath = osenvironb.get(IMPL_PATH)
        if oldpypath:
            pypath.append(oldpypath)
        osenvironb[IMPL_PATH] = sepb.join(pypath)

        if self.options.pure:
            os.environ["HGTEST_RUN_TESTS_PURE"] = "--pure"
            os.environ["HGMODULEPOLICY"] = "py"

        if self.options.allow_slow_tests:
            os.environ["HGTEST_SLOW"] = "slow"
        elif "HGTEST_SLOW" in os.environ:
            del os.environ["HGTEST_SLOW"]

        self._coveragefile = os.path.join(self._testdir, b".coverage")

        if self.options.exceptions:
            exceptionsdir = os.path.join(self._outputdir, b"exceptions")
            try:
                os.makedirs(exceptionsdir)
            except OSError as e:
                if e.errno != errno.EEXIST:
                    raise

            # Remove all existing exception reports.
            for f in os.listdir(exceptionsdir):
                os.unlink(os.path.join(exceptionsdir, f))

            osenvironb[b"HGEXCEPTIONSDIR"] = exceptionsdir
            logexceptions = os.path.join(self._testdir, b"logexceptions.py")
            self.options.extra_config_opt.append(
                "extensions.logexceptions=%s" % logexceptions.decode("utf-8")
            )

        vlog("# Using TESTDIR", self._testdir)
        vlog("# Using RUNTESTDIR", osenvironb[b"RUNTESTDIR"])
        vlog("# Using HGTMP", self._hgtmp)
        vlog("# Using PATH", os.environ["PATH"])
        vlog("# Using", IMPL_PATH, osenvironb[IMPL_PATH])
        if self._watchman:
            vlog("# Using watchman", self._watchman)
        vlog("# Writing to directory", self._outputdir)

        try:
            return self._runtests(testdescs) or 0
        finally:
            time.sleep(0.1)
            self._cleanup()

    def findtests(self, args):
        """Finds possible test files from arguments.

        If you wish to inject custom tests into the test harness, this would
        be a good function to monkeypatch or override in a derived class.
        """
        if not args:
            if self.options.changed:
                proc = Popen4(
                    'hg st --rev "%s" -man0 .' % self.options.changed, None, 0
                )
                stdout, stderr = proc.communicate()
                args = stdout.strip(b"\0").split(b"\0")
            else:
                args = os.listdir(b".")

        expanded_args = []
        for arg in args:
            if os.path.isdir(arg):
                if not arg.endswith(b"/"):
                    arg += b"/"
                expanded_args.extend([arg + a for a in os.listdir(arg)])
            else:
                expanded_args.append(arg)
        args = expanded_args

        tests = []
        for t in args:
            if not (
                os.path.basename(t).startswith(b"test-")
                and (t.endswith(b".py") or t.endswith(b".t"))
            ):
                continue
            if t.endswith(b".t"):
                # .t file may contain multiple test cases
                cases = sorted(parsettestcases(t))
                if cases:
                    tests += [{"path": t, "case": c} for c in sorted(cases)]
                else:
                    tests.append({"path": t})
            else:
                tests.append({"path": t})
        return tests

    def _runtests(self, testdescs):
        def _reloadtest(test, i):
            # convert a test back to its description dict
            desc = {"path": test.path}
            case = getattr(test, "_case", None)
            if case:
                desc["case"] = case
            return self._gettest(desc, i)

        failed = False
        allskipped = False
        errored = False
        try:
            if self.options.restart:
                orig = list(testdescs)
                while testdescs:
                    desc = testdescs[0]
                    # desc['path'] is a relative path
                    if "case" in desc:
                        errpath = b"%s.%s.err" % (desc["path"], desc["case"])
                    else:
                        errpath = b"%s.err" % desc["path"]
                    errpath = os.path.join(self._outputdir, errpath)
                    if os.path.exists(errpath):
                        break
                    testdescs.pop(0)
                if not testdescs:
                    print("running all tests")
                    testdescs = orig

            tests = [self._gettest(d, i) for i, d in enumerate(testdescs)]

            kws = self.options.keywords
            if kws is not None and PYTHON3:
                kws = kws.encode("utf-8")

            vlog("# Running TestSuite with %d jobs" % self.options.jobs)
            suite = TestSuite(
                self._testdir,
                jobs=self.options.jobs,
                whitelist=self.options.whitelisted,
                blacklist=self.options.blacklist,
                retest=self.options.retest,
                keywords=kws,
                loop=self.options.loop,
                runs_per_test=self.options.runs_per_test,
                showchannels=self.options.showchannels,
                tests=tests,
                loadtest=_reloadtest,
            )
            verbosity = 1
            if self.options.verbose:
                verbosity = 2

            if self.options.testpilot:
                runner = TestpilotTestRunner(self)
            else:
                runner = TextTestRunner(self, verbosity=verbosity)

            if self.options.list_tests:
                result = runner.listtests(suite)
            else:
                if self._installdir:
                    self._installhg()
                    self._checkhglib("Testing")
                else:
                    self._usecorrectpython()
                if self.options.chg:
                    assert self._installdir
                    self._installchg()

                self._usecorrecthg()

                result = runner.run(suite)
                if tests and result.testsSkipped == len(tests):
                    allskipped = True
                if tests and result.errors:
                    errored = True

            if result.failures:
                failed = True

            if self.options.anycoverage:
                self._outputcoverage()
        except KeyboardInterrupt:
            failed = True
            print("\ninterrupted!")

        if failed:
            return 1
        elif allskipped:
            return 0
        elif errored:
            return 2

    def _getport(self, count):
        port = self._ports.get(count)  # do we have a cached entry?
        if port is None:
            portneeded = 3
            # above 100 tries we just give up and let test reports failure
            for tries in xrange(100):
                allfree = True
                port = self.options.port + self._portoffset
                for idx in xrange(portneeded):
                    if not checkportisavailable(port + idx):
                        allfree = False
                        break
                self._portoffset += portneeded
                if allfree:
                    break
            self._ports[count] = port
        return port

    def _gettest(self, testdesc, count):
        """Obtain a Test by looking at its filename.

        Returns a Test instance. The Test may not be runnable if it doesn't
        map to a known type.
        """
        path = testdesc["path"]
        lctest = path.lower()
        testcls = Test

        for ext, cls in self.TESTTYPES:
            if lctest.endswith(ext):
                testcls = cls
                break

        refpath = os.path.join(self._testdir, path)
        tmpdir = os.path.join(self._hgtmp, b"child%d" % count)

        # extra keyword parameters. 'case' is used by .t tests
        kwds = dict((k, testdesc[k]) for k in ["case"] if k in testdesc)

        t = testcls(
            refpath,
            self._outputdir,
            tmpdir,
            keeptmpdir=self.options.keep_tmpdir,
            debug=self.options.debug,
            first=self.options.first,
            timeout=self.options.timeout,
            startport=self._getport(count),
            extraconfigopts=self.options.extra_config_opt,
            extrarcpaths=self.options.extra_rcpath,
            py3kwarnings=self.options.py3k_warnings,
            shell=self.options.shell,
            hgcommand=self._hgcommand,
            usechg=bool(self.options.with_chg or self.options.chg),
            useipv6=useipv6,
            watchman=self._watchman,
            **kwds
        )
        t.should_reload = True
        return t

    def _cleanup(self):
        """Clean up state from this test invocation."""
        if self.options.keep_tmpdir:
            return

        vlog("# Cleaning up HGTMP", self._hgtmp)
        shutil.rmtree(self._hgtmp, True)
        for f in self._createdfiles:
            try:
                os.remove(f)
            except OSError:
                pass

    def _usecorrectpython(self):
        """Configure the environment to use the appropriate Python in tests."""
        # Tests must use the same interpreter as us or bad things will happen.
        pyexename = sys.platform == "win32" and b"python.exe" or b"python"
        if getattr(os, "symlink", None):
            vlog(
                "# Making python executable in test path a symlink to '%s'"
                % sys.executable
            )
            mypython = os.path.join(self._tmpbindir, pyexename)
            try:
                if os.readlink(mypython) == sys.executable:
                    return
                os.unlink(mypython)
            except OSError as err:
                if err.errno != errno.ENOENT:
                    raise
            if self._findprogram(pyexename) != sys.executable:
                try:
                    os.symlink(sys.executable, mypython)
                    self._createdfiles.append(mypython)
                except OSError as err:
                    # child processes may race, which is harmless
                    if err.errno != errno.EEXIST:
                        raise
        else:
            exedir, exename = os.path.split(sys.executable)
            vlog(
                "# Modifying search path to find %s as %s in '%s'"
                % (exename, pyexename, exedir)
            )
            path = os.environ["PATH"].split(os.pathsep)
            while exedir in path:
                path.remove(exedir)
            os.environ["PATH"] = os.pathsep.join([exedir] + path)
            if not self._findprogram(pyexename):
                print("WARNING: Cannot find %s in search path" % pyexename)

    def _usecorrecthg(self):
        """Configure the environment to use the appropriate hg in tests."""
        if os.path.basename(self._hgcommand) in ("hg", "hg.exe"):
            # No correction is needed
            return
        if getattr(os, "symlink", None):
            tmphgpath = os.path.join(self._tmpbindir, "hg")
            vlog("# Symlink %s to %s" % (self._hgcommand, tmphgpath))
            entrypointpath = os.path.join(
                os.path.dirname(os.path.realpath(self._hgcommand)),
                "mercurial",
                "entrypoint.py",
            )
            if os.path.exists(entrypointpath):
                vlog("# HGPYENTRYPOINT=%s" % entrypointpath)
                os.environ["HGPYENTRYPOINT"] = entrypointpath

            try:
                os.symlink(self._hgcommand, tmphgpath)
                self._createdfiles.append(tmphgpath)
            except OSError as err:
                # child processes may race, which is harmless
                if err.errno != errno.EEXIST:
                    raise
        else:
            raise SystemExit("%s could not be put in search path" % self._hgcommand)

    def _installhg(self):
        """Install hg into the test environment.

        This will also configure hg with the appropriate testing settings.
        """
        vlog("# Performing temporary installation of HG")
        installerrs = os.path.join(self._hgtmp, b"install.err")
        compiler = ""
        if self.options.compiler:
            compiler = "--compiler " + self.options.compiler
        if self.options.pure:
            pure = b"--pure"
        else:
            pure = b""

        # Run installer in hg root
        script = os.path.realpath(sys.argv[0])
        exe = sys.executable
        if PYTHON3:
            compiler = _bytespath(compiler)
            script = _bytespath(script)
            exe = _bytespath(exe)
        hgroot = os.path.dirname(os.path.dirname(script))
        self._hgroot = hgroot
        os.chdir(hgroot)
        nohome = b'--home=""'
        if os.name == "nt":
            # The --home="" trick works only on OS where os.sep == '/'
            # because of a distutils convert_path() fast-path. Avoid it at
            # least on Windows for now, deal with .pydistutils.cfg bugs
            # when they happen.
            nohome = b""
        cmd = (
            b"%(exe)s setup.py %(pure)s clean --all"
            b' build %(compiler)s --build-base="%(base)s"'
            b' install --force --prefix="%(prefix)s"'
            b' --install-lib="%(libdir)s"'
            b' --install-scripts="%(bindir)s" %(nohome)s >%(logfile)s 2>&1'
            % {
                b"exe": exe,
                b"pure": pure,
                b"compiler": compiler,
                b"base": os.path.join(self._hgtmp, b"build"),
                b"prefix": self._installdir,
                b"libdir": self._pythondir,
                b"bindir": self._bindir,
                b"nohome": nohome,
                b"logfile": installerrs,
            }
        )

        # setuptools requires install directories to exist.
        def makedirs(p):
            try:
                os.makedirs(p)
            except OSError as e:
                if e.errno != errno.EEXIST:
                    raise

        makedirs(self._pythondir)
        makedirs(self._bindir)

        vlog("# Running", cmd)
        if os.system(cmd) == 0:
            if not self.options.verbose:
                try:
                    os.remove(installerrs)
                except OSError as e:
                    if e.errno != errno.ENOENT:
                        raise
        else:
            with open(installerrs, "rb") as f:
                for line in f:
                    if PYTHON3:
                        sys.stdout.buffer.write(line)
                    else:
                        sys.stdout.write(line)
            sys.exit(1)
        os.chdir(self._testdir)

        self._usecorrectpython()

        if self.options.py3k_warnings and not self.options.anycoverage:
            vlog("# Updating hg command to enable Py3k Warnings switch")
            with open(os.path.join(self._bindir, "hg"), "rb") as f:
                lines = [line.rstrip() for line in f]
                lines[0] += " -3"
            with open(os.path.join(self._bindir, "hg"), "wb") as f:
                for line in lines:
                    f.write(line + "\n")

        hgbat = os.path.join(self._bindir, b"hg.bat")
        if os.path.isfile(hgbat):
            # hg.bat expects to be put in bin/scripts while run-tests.py
            # installation layout put it in bin/ directly. Fix it
            with open(hgbat, "rb") as f:
                data = f.read()
            if b'"%~dp0..\\python" "%~dp0hg" %*' in data:
                data = data.replace(
                    b'"%~dp0..\\python" "%~dp0hg" %*', b'"%~dp0python" "%~dp0hg" %*'
                )
                with open(hgbat, "wb") as f:
                    f.write(data)
            else:
                print("WARNING: cannot fix hg.bat reference to python.exe")

        if self.options.anycoverage:
            custom = os.path.join(self._testdir, "sitecustomize.py")
            target = os.path.join(self._pythondir, "sitecustomize.py")
            vlog("# Installing coverage trigger to %s" % target)
            shutil.copyfile(custom, target)
            rc = os.path.join(self._testdir, ".coveragerc")
            vlog("# Installing coverage rc to %s" % rc)
            os.environ["COVERAGE_PROCESS_START"] = rc
            covdir = os.path.join(self._installdir, "..", "coverage")
            try:
                os.mkdir(covdir)
            except OSError as e:
                if e.errno != errno.EEXIST:
                    raise

            os.environ["COVERAGE_DIR"] = covdir

    def _checkhglib(self, verb):
        """Ensure that the 'mercurial' package imported by python is
        the one we expect it to be.  If not, print a warning to stderr."""
        if (self._bindir == self._pythondir) and (self._bindir != self._tmpbindir):
            # The pythondir has been inferred from --with-hg flag.
            # We cannot expect anything sensible here.
            return
        expecthg = os.path.join(self._pythondir, b"edenscm", b"mercurial")
        actualhg = self._gethgpath()
        if os.path.abspath(actualhg) != os.path.abspath(expecthg):
            sys.stderr.write(
                "warning: %s with unexpected mercurial lib: %s\n"
                "         (expected %s)\n" % (verb, actualhg, expecthg)
            )

    def _gethgpath(self):
        """Return the path to the mercurial package that is actually found by
        the current Python interpreter."""
        if self._hgpath is not None:
            return self._hgpath

        cmd = b'%s -c "from edenscm import mercurial; print (mercurial.__path__[0])"'
        cmd = cmd % PYTHON
        if PYTHON3:
            cmd = _strpath(cmd)
        pipe = os.popen(cmd)
        try:
            self._hgpath = _bytespath(pipe.read().strip())
        finally:
            pipe.close()

        return self._hgpath

    def _installchg(self):
        """Install chg into the test environment"""
        vlog("# Performing temporary installation of CHG")
        assert os.path.dirname(self._bindir) == self._installdir
        assert self._hgroot, "must be called after _installhg()"
        cmd = b'"%(make)s" clean install PREFIX="%(prefix)s"' % {
            b"make": "make",  # TODO: switch by option or environment?
            b"prefix": self._installdir,
        }
        cwd = os.path.join(self._hgroot, b"contrib", b"chg")
        vlog("# Running", cmd)
        proc = subprocess.Popen(
            cmd,
            shell=True,
            cwd=cwd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        out, _err = proc.communicate()
        if proc.returncode != 0:
            if PYTHON3:
                sys.stdout.buffer.write(out)
            else:
                sys.stdout.write(out)
            sys.exit(1)

    def _outputcoverage(self):
        """Produce code coverage output."""
        import coverage

        coverage = coverage.coverage

        vlog("# Producing coverage report")
        # chdir is the easiest way to get short, relative paths in the
        # output.
        os.chdir(self._hgroot)
        covdir = os.path.join(self._installdir, "..", "coverage")
        cov = coverage(data_file=os.path.join(covdir, "cov"))

        # Map install directory paths back to source directory.
        cov.config.paths["srcdir"] = [".", self._pythondir]

        cov.combine()

        omit = [os.path.join(x, "*") for x in [self._bindir, self._testdir]]
        cov.report(ignore_errors=True, omit=omit)

        if self.options.htmlcov:
            htmldir = os.path.join(self._outputdir, "htmlcov")
            cov.html_report(directory=htmldir, omit=omit)
        if self.options.annotate:
            adir = os.path.join(self._outputdir, "annotated")
            if not os.path.isdir(adir):
                os.mkdir(adir)
            cov.annotate(directory=adir, omit=omit)

    def _findprogram(self, program):
        """Search PATH for a executable program"""
        dpb = _bytespath(os.defpath)
        sepb = _bytespath(os.pathsep)
        for p in osenvironb.get(b"PATH", dpb).split(sepb):
            name = os.path.join(p, program)
            if os.name == "nt" or os.access(name, os.X_OK):
                return name
        return None

    def _checktools(self):
        """Ensure tools required to run tests are present."""
        for p in self.REQUIREDTOOLS:
            if os.name == "nt" and not p.endswith(".exe"):
                p += ".exe"
            found = self._findprogram(p)
            if found:
                vlog("# Found prerequisite", p, "at", found)
            else:
                print(
                    "WARNING: Did not find prerequisite tool: %s " % p.decode("utf-8")
                )


def aggregateexceptions(path):
    exceptions = collections.Counter()

    for f in os.listdir(path):
        with open(os.path.join(path, f), "rb") as fh:
            data = fh.read().split(b"\0")
            if len(data) != 4:
                continue

            exc, mainframe, hgframe, hgline = data
            exc = exc.decode("utf-8")
            mainframe = mainframe.decode("utf-8")
            hgframe = hgframe.decode("utf-8")
            hgline = hgline.decode("utf-8")
            exceptions[(hgframe, hgline, exc)] += 1

    return exceptions


def ensureenv():
    """Load build/env's environment variables.

    If build/env has specified a different set of environment variables,
    restart the current command. Otherwise do nothing.
    """
    hgdir = os.path.dirname(os.path.dirname(os.path.realpath(__file__)))
    envpath = os.path.join(hgdir, "build", "env")
    if not os.path.exists(envpath):
        return
    with open(envpath, "r") as f:
        env = dict(l.split("=", 1) for l in f.read().splitlines() if "=" in l)
    if all(os.environ.get(k) == v for k, v in env.items()):
        # No restart needed
        return
    # Restart with new environment
    newenv = os.environ.copy()
    newenv.update(env)
    # Pick the right Python interpreter
    python = env.get("PYTHON_SYS_EXECUTABLE", sys.executable)
    p = subprocess.Popen([python] + sys.argv, env=newenv)
    sys.exit(p.wait())


if __name__ == "__main__":
    ensureenv()
    runner = TestRunner()

    try:
        import msvcrt

        msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)
        msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
        msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
    except ImportError:
        pass

    sys.exit(runner.run(sys.argv[1:]))
