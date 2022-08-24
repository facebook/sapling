# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Translate run-tests.py tests to Python standard unittests

import contextlib
import glob
import os
import random
import re
import shlex
import subprocess
import sys
import unittest

import libfb.py.parutil as parutil


try:
    # Used by :run_tests binary target
    import libfb.py.pathutils as pathutils

    hgpath = parutil.get_file_path("hg.sh", pkg=__package__)
    pythonbinpath = parutil.get_file_path("hgpython.sh", pkg=__package__)
    watchman = parutil.get_file_path("watchman", pkg=__package__)
    mononoke_server = parutil.get_file_path("mononoke", pkg=__package__)
    dummyssh = parutil.get_file_path("dummyssh3.par", pkg=__package__)
    get_free_socket = parutil.get_file_path("get_free_socket.par", pkg=__package__)
except ImportError:
    # Used by :hg_run_tests and :hg_watchman_run_tests unittest target
    hgpath = os.environ.get("HGTEST_HG")
    pythonbinpath = os.environ.get("HGTEST_PYTHON", "python2")
    watchman = os.environ.get("HGTEST_WATCHMAN")
    mononoke_server = os.environ.get("HGTEST_MONONOKE_SERVER")
    dummyssh = os.environ.get("HGTEST_DUMMYSSH")
    get_free_socket = os.environ.get("HGTEST_GET_FREE_SOCKET")


if watchman is not None and not os.path.exists(str(watchman)):
    watchman = None

os.environ["PYTHON_SYS_EXECUTABLE"] = pythonbinpath

try:
    shlex_quote = shlex.quote  # Python 3.3 and up
except AttributeError:
    # pyre-fixme[9]: shlex_quote has type `(s: str) -> str`; used as `(seq:
    #  Sequence[str]) -> str`.
    # pyre-fixme[9]: shlex_quote has type `(s: str) -> str`; used as `(seq:
    #  Sequence[str]) -> str`.
    shlex_quote = subprocess.list2cmdline


@contextlib.contextmanager
def chdir(path):
    oldpwd = os.getcwd()
    try:
        os.chdir(path)
        yield
    finally:
        os.chdir(oldpwd)


def prepareargsenv(runtestsdir, port=None):
    """return (args, env) for running run-tests.py"""
    if not os.path.exists(os.path.join(runtestsdir, "run-tests.py")):
        raise SystemExit("cannot find run-tests.py from %s" % runtestsdir)
    env = os.environ.copy()
    args = [pythonbinpath, "run-tests.py", "--maxdifflines=1000"]
    if port:
        args += ["--port", "%s" % port]

    if hgpath:
        args.append("--with-hg=%s" % hgpath)
    if watchman:
        args += ["--with-watchman", watchman]
    # set HGDATAPATH
    datapath = os.path.join(runtestsdir, "../edenscm")
    env["HGDATAPATH"] = datapath
    # set HGPYTHONPATH since PYTHONPATH might be discarded
    pythonpath = os.pathsep.join([runtestsdir])
    env["HGPYTHONPATH"] = pythonpath
    # set other environments useful for buck testing
    env["HGTEST_NORMAL_LAYOUT"] = "0"
    if dummyssh is not None:
        env["DUMMYSSH"] = dummyssh

    # Variables needed for mononoke integration
    if os.environ.get("USE_MONONOKE"):
        env["MONONOKE_SERVER"] = mononoke_server
        env["GET_FREE_SOCKET"] = get_free_socket

    return args, env


def gettestmethod(name, port):
    def runsingletest(self):
        reportskips = os.getenv("HGTEST_REPORT_SKIPS")
        with chdir(self._runtests_dir):
            args, env = prepareargsenv(self._runtests_dir, port)
            args += os.getenv("HGTEST_RUNTESTS_ARGS", "").split()
            # run run-tests.py for a single test
            p = subprocess.Popen(
                args + [name], env=env, stderr=subprocess.PIPE, stdout=subprocess.PIPE
            )
            out, err = p.communicate("")
            message = err + out
            returncode = p.returncode
            if b"Lost connection to MySQL server" in message:
                raise unittest.SkipTest("MySQL is unavailable")
            if returncode == 80:
                if not reportskips:
                    return
                # Extract skipped reason from output
                match = re.search(b"Skipped [^:]*: (.*)", message)
                if match:
                    reason = match.group(1)
                else:
                    reason = b"skipped by run-tests.py"
                raise unittest.SkipTest(reason)
            elif returncode != 0:
                raise self.failureException(
                    message.decode("utf-8", errors="surrogateescape")
                )

    return runsingletest


class hgtests(unittest.TestCase):
    @classmethod
    def collecttests(cls, path):
        """scan tests in path and add them as test methods"""
        if os.environ.get("HGTEST_IGNORE_INCLUDED") == "1":
            included = None
        else:
            included = re.compile(r"\A%s\Z" % os.environ.get("HGTEST_INCLUDED", ".*"))

        if os.environ.get("HGTEST_IGNORE_EXCLUDED") == "1":
            excluded = None
        else:
            excluded = re.compile(r"\A%s\Z" % os.environ.get("HGTEST_EXCLUDED", ""))
        # Randomize the port so a stress run of a single test would be fine
        port = random.randint(10000, 60000)
        with chdir(path):
            cls._runtests_dir = os.getcwd()
            for name in glob.glob("test-*.t") + glob.glob("test-*.py"):
                method_name = name.replace(".", "_").replace("-", "_")
                if included and not included.match(method_name):
                    continue
                if excluded and excluded.match(method_name):
                    continue
                # Running a lot run-tests.py in parallel will trigger race
                # condition of the original port detection logic. So allocate
                # ports here. run-tests.py could do adjustments.
                # A test needs 3 ports at most. See portneeded in run-tests.py
                port += 3
                setattr(cls, method_name, gettestmethod(name, port))


if __name__ == "__main__":
    args, env = prepareargsenv(os.getcwd())
    os.execvpe(args[0], args + sys.argv[1:], env)
else:
    hgtests.collecttests(os.environ.get("HGTEST_DIR", "."))
