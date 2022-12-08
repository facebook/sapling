# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""hg extension for TestTmp

- register the "hg" command
- sets HGRCPATH
- sets hg related environ variables
- source tinit.sh
"""

import io
import os
import subprocess
import sys
from functools import partial
from typing import BinaryIO

from ..sh import Env, Scope
from ..sh.interp import interpcode
from ..t.runtime import TestTmp
from ..t.shext import shellenv
from .python import python


def testsetup(t: TestTmp):
    _checkenvironment()

    hgrc = INITIAL_HGRC

    # consider run-tests.py --watchman
    use_watchman = os.getenv("HGFSMONITOR_TESTS") == "1"
    if use_watchman:
        hgrc += WATCHMAN_HGRC

    hgrcpath = t.path / "hgrc"
    hgrcpath.write_bytes(hgrc)

    # extra hgrc fixup via $TESTDIR/features.py
    testfile = t.getenv("TESTFILE")
    testdir = t.getenv("TESTDIR")
    featurespy = os.path.join(testdir, "features.py")

    if os.path.exists(featurespy):
        with open(featurespy, "r") as f:
            globalenv = {}
            exec(f.read(), globalenv)
            setup = globalenv.get("setup")
            if setup:
                testname = os.path.basename(testfile)
                setup(testname, str(hgrcpath))
    inprocesshg = True
    if os.path.exists(testfile):
        with open(testfile, "rb") as f:
            content = f.read().decode("utf-8", errors="ignore")
        if "#inprocess-hg-incompatible" in content:
            inprocesshg = False

    # the 'f' utility in $TESTDIR/f
    fpath = os.path.join(testdir, "f")
    if os.path.exists(fpath):
        fmain = None

        @t.command
        def f(args, stdout, stdin, fs, fpath=fpath) -> int:
            nonlocal fmain
            if fmain is None:
                fmain = _execpython(fpath)["main"]
            os.chdir(fs.cwd())
            try:
                fmain(args, stdout=stdout, stdin=stdin)
            except SystemExit as e:
                return int(e.code)
            else:
                return 0

    # extra pattern substitutions in $TESTDIR/common-pattern.py
    fpath = os.path.join(testdir, "common-pattern.py")
    if os.path.exists(fpath):
        t.substitutions += _execpython(fpath).get("substitutions") or []
    t.substitutions += [
        (r"\bHG_TXNID=TXN:[a-f0-9]{40}\b", r"HG_TXNID=TXN:$ID$"),
    ]

    environ = {
        "CHGDISABLE": "0",
        "COLUMNS": "80",
        "DAEMON_PIDS": str(t.path / "daemon.pids"),
        "EMAIL": "Foo Bar <foo.bar@example.com>",
        "HGCOLORS": "16",
        "HGEDITOR": "internal:none",
        "HGEMITWARNINGS": "1",
        "HGENCODINGMODE": "strict",
        "HGENCODING": "utf-8",
        "HGMERGE": "internal:merge",
        "HGOUTPUTENCODING": "utf-8",
        "HGRCPATH": str(hgrcpath),
        "HGUSER": "test",
        "LANG": "en_US.UTF-8",
        "LANGUAGE": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "LOCALIP": "127.0.0.1",
        "SL_CONFIG_PATH": str(hgrcpath),
        # Normalize TERM to avoid control sequence variations.
        # We use a non-existent terminal to avoid any terminfo dependency.
        "TERM": "fake-term",
        "TZ": "GMT",
    }

    if use_watchman:
        watchman_sock = os.getenv("WATCHMAN_SOCK")
        if watchman_sock:
            environ["WATCHMAN_SOCK"] = watchman_sock
            environ["HGFSMONITOR_TESTS"] = "1"

    # prepare chg
    with open(testfile, "rb") as f:
        header = f.read(256)
        usechg = b"#chg-compatible" in header
    if usechg:
        environ["CHGDISABLE"] = "never"
        environ["CHGSOCKNAME"] = str(t.path / "chgserver")
    else:
        environ["CHGDISABLE"] = "1"

    for k, v in environ.items():
        t.setenv(k, v)

    # source tinit.sh
    tinitpath = os.path.join(testdir, "tinit.sh")
    if os.path.exists(tinitpath):
        with open(tinitpath, "rb") as f:
            t.sheval(f.read().decode())

    hgpath = None
    run = None
    try:
        import bindings
        from edenscm import util

        run = bindings.commands.run
    except ImportError:
        hgpath = os.environ.get("HG")
        if hgpath and not os.path.exists(hgpath):
            hgpath = None
    else:
        hgpath = util.hgcmd()[0]

    if hgpath:
        # provide access to the real binary
        t.requireexe("hg", hgpath)
        # required for util.hgcmd to work in buck test, where
        # the entry point is a shell script, not argv[0].
        t.setenv("HGEXECUTABLEPATH", hgpath)

    # change the 'hg' shell command to run inline without spawning
    # (about 2x faster than chg)
    if run is not None and inprocesshg:
        t.command(hg)


def _checkenvironment():
    """check the python global state is clean"""
    # - "edenscm.dispatch" module is not yet imported. This happens if run via
    #   'hg debugpython' with chg disabled, or via vanilla 'python' - okay.
    # - "edenscm.dispatch" module is imported, and ischgserver is True.
    #   chgserver preimports modules but does not call uisetup()s, so it's okay.
    # - "edenscm.dispatch" module is imported, and ischgserver is False.
    #   This is the regular "hg" command path. It's not okay since uisetup()s
    #   might be called and Python global state is no longer clean.
    mod = sys.modules.get("edenscm.dispatch")
    assert (
        mod is None or mod.ischgserver
    ), "testing should not be run under regular edenscm environment"


def hg(stdin: BinaryIO, stdout: BinaryIO, stderr: BinaryIO, env: Env) -> int:
    """run hg commands in-process
    requires edenscm modules - run from "hg debugpython", not vanilla python
    """
    # debugpython won't work - emulate Py_Main instead
    if env.args[1:3] == ["debugpython", "--"]:
        env.args = [env.args[0]] + env.args[3:]
        args = env.args[1:]
        return python(args, stdin, stdout, stderr, env)

    import bindings
    from edenscm import encoding, extensions, pycompat, util

    # emulate ui.system via sheval
    rawsystem = partial(_rawsystem, env, stdin, stdout, stderr)
    origstdio = (pycompat.stdin, pycompat.stdout, pycompat.stderr)

    # bindings.commands.run might keep the stdio strems to prevent
    # file delection (for example, if stdout redirects to a file).
    # Workaround that by using a temporary in-memory stream.
    if os.name == "nt":
        real_stdout, real_stderr = stdout, stderr
        stdout = io.BytesIO()
        if real_stdout is real_stderr:
            stderr = stdout
        else:
            stderr = io.BytesIO()

    try:
        with shellenv(
            env, stdin=stdin, stdout=stdout, stderr=stderr
        ), extensions.wrappedfunction(
            util, "rawsystem", rawsystem
        ), extensions.wrappedfunction(
            subprocess, "run", _patchedsubprun
        ):
            bindings.identity.resetdefault()

            encoding.setfromenviron()
            pycompat.stdin = stdin
            pycompat.stdout = stdout
            pycompat.stderr = stderr
            pycompat.sysargv = env.args
            util._reloadenv()
            exitcode = bindings.commands.run(env.args, stdin, stdout, stderr)
            return exitcode
    finally:
        # See above. This avoids leaking file descriptions that prevents
        # file deletion on Windows.
        if os.name == "nt":
            # pyre-fixme[16]: `BinaryIO` has no attribute `getvalue`.
            real_stdout.write(stdout.getvalue())
            real_stdout.flush()
            # pyre-fixme[61]: `real_stdout` is undefined, or not always defined.
            # pyre-fixme[61]: `real_stderr` is undefined, or not always defined.
            if real_stdout is not real_stderr:
                real_stderr.write(stderr.getvalue())
                real_stderr.flush()
            # release blackbox for deletion
            bindings.blackbox.reset()
        # restore environ
        encoding.setfromenviron()
        pycompat.stdin, pycompat.stdout, pycompat.stderr = origstdio


def _rawsystem(
    shenv, stdin, stdout, stderr, orig, cmd: str, environ=None, cwd=None, out=None
):
    # use testing.sh.interpcode to run the command
    env = shenv.nested(Scope.SHELL)
    env.stdin = stdin
    env.stdout = out or stdout
    env.stderr = stderr
    env.fs.chdir(cwd or os.getcwd())
    if environ is not None:
        for k, v in environ.items():
            env.setenv(k, str(v), Scope.SHELL)
            env.exportenv(k)
    res = interpcode(cmd, env)
    if res.out:
        env.stdout.write(res.out.encode())
    return res.exitcode


def _patchedsubprun(orig, args, **kwargs):
    if os.name == "nt" and args:
        # append ".bat" to args[0]
        arg0bat = f"{args[0]}.bat"
        paths = os.getenv("PATH", "").split(os.pathsep)
        if any(os.path.exists(os.path.join(p, arg0bat)) for p in paths):
            args[0] = arg0bat
    return orig(args, **kwargs)


def _execpython(path):
    """execute python code at path, return its globals"""
    with open(path, "r") as f:
        src = f.read()
    code = compile(src, path, "exec")
    env = {}
    exec(code, env)
    return env


INITIAL_HGRC = b"""
[ui]
slash = True
interactive = False
mergemarkers = detailed
promptecho = True

[devel]
all-warnings = true
collapse-traceback = true
default-date = 0 0

[web]
address = localhost
ipv6 = False

[workingcopy]
enablerustwalker=True

[extensions]
treemanifest=

[treemanifest]
sendtrees=True
treeonly=True
rustmanifest=True
useruststore=True

[remotefilelog]
reponame=reponame-default
localdatarepack=True
cachepath=$TESTTMP/default-hgcache

[mutation]
record=False

[hint]
ack-match-full-traversal=True
"""

WATCHMAN_HGRC = b"""
[extensions]
fsmonitor=

[fsmonitor]
detectrace=True
"""
