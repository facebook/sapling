from __future__ import absolute_import

import multiprocessing
import os
import subprocess


reporoot = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def getrunner():
    """return the path of run-tests.py. best-effort"""
    runner = "run-tests.py"
    # Try MERCURIALRUNTEST
    candidate = os.environ.get("MERCURIALRUNTEST")
    if candidate and os.access(candidate, os.X_OK):
        return candidate
    # Search in PATH
    for d in os.environ.get("PATH").split(os.path.pathsep):
        candidate = os.path.abspath(os.path.join(d, runner))
        if os.access(candidate, os.X_OK):
            return candidate
    # Search some common places for run-tests.py, as a nice default
    # if we cannot find it otherwise.
    for prefix in [os.path.dirname(reporoot), os.path.expanduser("~")]:
        for hgrepo in ["hg", "hg-crew", "hg-committed"]:
            path = os.path.abspath(os.path.join(prefix, hgrepo, "tests", runner))
            if os.access(path, os.X_OK):
                return path
    return runner


def reporequires():
    """return a list of string, which are the requirements of the hg repo"""
    requirespath = os.path.join(reporoot, ".hg", "requires")
    if os.path.exists(requirespath):
        return [s.rstrip() for s in open(requirespath, "r")]
    return []


def spawnruntests(args, **kwds):
    cpucount = multiprocessing.cpu_count()
    cmd = [getrunner(), "-j%d" % cpucount]

    # Include the repository root in PYTHONPATH so the unit tests will find
    # the extensions from the local repository, rather than the versions
    # already installed on the system.
    env = os.environ.copy()
    if "PYTHONPATH" in env:
        existing_pypath = [env["PYTHONPATH"]]
    else:
        existing_pypath = []
    env["PYTHONPATH"] = os.path.pathsep.join([reporoot] + existing_pypath)

    # Spawn the run-tests.py process.
    cmd += args
    cwd = os.path.join(reporoot, "tests")
    proc = subprocess.Popen(cmd, cwd=cwd, env=env, **kwds)
    return proc
