# Translate run-tests.py tests to Python standard unittests
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import, division, print_function, unicode_literals

import contextlib
import glob
import os
import random
import re
import shlex
import subprocess
import sys
import unittest


try:
    import libfb.py.pathutils as pathutils

    chgpath = pathutils.get_build_rule_output_path("//scm/hg:chg")
    hgpath = pathutils.get_build_rule_output_path("//scm/hg:hg")
    pythonbinpath = pathutils.get_build_rule_output_path("//scm/hg:hgpython")
except ImportError:
    chgpath = os.environ.get("HGTEST_CHG")
    hgpath = os.environ.get("HGTEST_HG")
    pythonbinpath = os.environ.get("HGTEST_PYTHON", "python2")


try:
    shlex_quote = shlex.quote  # Python 3.3 and up
except AttributeError:
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
    args = [pythonbinpath, "run-tests.py"]
    if port:
        args += ["--port", "%s" % port]

    if hgpath:
        args.append("--with-hg=%s" % hgpath)
    if chgpath:
        env["CHG"] = chgpath
    # set HGDATAPATH
    datapath = os.path.join(runtestsdir, "../mercurial")
    env["HGDATAPATH"] = datapath
    # set HGPYTHONPATH since PYTHONPATH might be discarded
    pythonpath = os.pathsep.join([runtestsdir])
    env["HGPYTHONPATH"] = pythonpath
    # set other environments useful for buck testing
    env["HGTEST_NORMAL_LAYOUT"] = "0"
    return args, env


def gettestmethod(name, port):
    def runsingletest(self):
        with chdir(self._runtests_dir):
            args, env = prepareargsenv(self._runtests_dir)
            args += os.getenv("HGTEST_RUNTESTS_ARGS", "").split()
            # run run-tests.py for a single test
            p = subprocess.Popen(
                args + [name], env=env, stderr=subprocess.PIPE, stdout=subprocess.PIPE
            )
            out, err = p.communicate("")
            returncode = p.returncode
            if returncode == 80:
                # Extract skipped reason from output
                match = re.search("Skipped [^:]*: (.*)", err + out)
                if match:
                    reason = match.group(1)
                else:
                    reason = "skipped by run-tests.py"
                raise unittest.SkipTest(reason)
            elif returncode != 0:
                raise self.failureException(err + out)

    return runsingletest


class hgtests(unittest.TestCase):
    @classmethod
    def collecttests(cls, path):
        """scan tests in path and add them as test methods"""
        if os.environ.get("HGTEST_IGNORE_BLACKLIST") == "1":
            blacklist = None
        else:
            blacklist = re.compile("\A%s\Z" % os.environ.get("HGTEST_BLACKLIST", ""))
        # Randomize the port so a stress run of a single test would be fine
        port = random.randint(10000, 60000)
        with chdir(path):
            cls._runtests_dir = os.getcwd()
            for name in glob.glob("test-*.t") + glob.glob("test-*.py"):
                method_name = name.replace(".", "_").replace("-", "_")
                if blacklist and blacklist.match(method_name):
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
