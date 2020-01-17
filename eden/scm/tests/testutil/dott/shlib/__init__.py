# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Shell commands emulator for .t test compatibility

This module only aims to support shell commands used in most tests. It does not
try to match all behavior of existing shell commands.
"""

from __future__ import absolute_import

import glob
import os
import shutil
import stat
import sys


def editsyspath(modname, relpaths):
    """Try to make sure a top-level Python module is in sys.path

    This is done by trying adding

        os.path.join(p, r for p in sys.path for r in relpaths)

    to sys.path to satisfy the need.
    """
    for path in sys.path:
        path = os.path.abspath(path)
        for relpath in relpaths:
            candidate = os.path.realpath(os.path.join(path, relpath))
            if os.path.exists(os.path.join(candidate, modname, "__init__.py")):
                if candidate not in sys.path:
                    sys.path.insert(0, candidate)
                return


# Use "edenscm" for shell utilities and main hg commands.
# Try to make "edenscm" and "edenscmnative" available.
#
# ".." works when:
# - sys.path includes "$TESTDIR" (because run-tests.py path is in $TESTDIR)
# - "$TESTDIR/.." has the desired modules (in-place local build)
#
# "../../.." works when:
# - `fb/packaging/build_nupkg.py --test` copies `tests/` to `build/embedded`
#
# But do not mess up sys.path if the modules are already importable.
# This is important in the buck test case.
# Soon, the only way to run this script is to via "hg debugpython" and
# all sys.path mess can be removed.
try:
    from edenscmnative import diffhelpers
    from edenscm import mercurial
except (AttributeError, ImportError):
    editsyspath("edenscm", ["..", "../../.."])


for candidate in sys.path:
    if os.path.exists(os.path.join(candidate, "hghave.py")):
        TESTDIR = os.path.abspath(candidate)
        break
else:
    raise RuntimeError("Cannot find TESTDIR")


try:
    from edenscm.mercurial import encoding, util, pycompat
    import bindings
except ImportError:
    raise RuntimeError("Cannot find edenscm")


# coreutils


def cat(*args, **kwargs):
    content = "".join(open(path).read() for path in args)
    stdin = kwargs.get("stdin")
    if stdin is not None:
        content += expandpath(stdin)
    return content


cd = os.chdir


def chmod(*args):
    mode = 0
    for arg in args:
        if arg == "+x":
            mode = 0o777
        elif arg == "-x":
            mode = 0o666
        elif arg[:1] in {"+", "-"}:
            raise NotImplementedError("chmod %s is unsupported" % arg)
        else:
            os.chmod(arg, mode)


def cp(*args):
    copydir = args[0] == "-R"
    if copydir:
        args = args[1:]
    src, dst = args
    if os.path.isdir(src):
        if not copydir:
            raise RuntimeError("cp dir without -R")
        if os.path.isdir(dst):
            dst = os.path.join(dst, os.path.basename(src))
        shutil.copytree(src, dst)
    else:
        shutil.copy2(src, dst)


def echo(*args):
    return " ".join(args) + "\n"


def head(*args, **kwargs):
    n, lines = _lines(*args, **kwargs)
    return "".join(lines[:n])


def ln(*args):
    if len(args) == 3:
        assert args[0] == "-s"
        src, dst = args[1:]
        os.symlink(src, dst)
    elif len(args) == 2:
        src, dst = args
        os.link(src, dst)
    else:
        raise NotImplementedError("ln with more args is not implemented")


def ls(*args):
    entries = []
    for arg in args:
        if arg.startswith("-"):
            raise NotImplementedError("ls with args is not implemented")
        elif os.path.isdir(arg):
            entries += os.listdir(arg)
        else:
            entries += glob.glob(arg)
    if not args:
        entries = os.listdir(".")
    return "".join("%s\n" % n for n in sorted(entries) if not n.startswith("."))


def mkdir(*args):
    for path in args:
        if path.startswith("-"):
            continue
        util.makedirs(path)


def mv(src, dst):
    shutil.move(src, dst)


def printf(*args):
    return args[0].replace(r"\n", "\n") % args[1:]


def rm(*args):
    for path in args:
        if path.startswith("-"):
            continue
        elif os.path.isdir(path):
            shutil.rmtree(path)
        else:
            util.tryunlink(path)


rmdir = rm


def seq(*args):
    if len(args) == 1:
        values = range(1, args[0] + 1)
    elif len(args) == 2:
        values = range(args[0], args[1] + 1)
    return "".join("%s\n" % v for v in values)


def tail(*args, **kwargs):
    n, lines = _lines(*args, **kwargs)
    return "".join(lines[-n:])


def test(*args):
    neg = False
    func = os.path.exists
    for arg in args:
        if arg == "!":
            neg = True
        elif arg == "-x":
            func = lambda p: os.stat(p).st_mode & stat.S_IEXEC
        elif arg == "-f":
            func = lambda p: stat.S_ISREG(os.stat(p).st_mode)
        elif arg == "-d":
            func = os.path.isdir
        else:
            result = func(arg)
            if neg:
                result = not result
            if result:
                return ""
            else:
                return "[1]"
    raise NotImplementedError("test not fully implemented")


def touch(*args):
    for path in args:
        open(path, "a")


def true():
    return ""


def wc(*args, **kwargs):
    stdin = kwargs.get("stdin", "")
    linecount = len(stdin.splitlines())
    for arg in args:
        if arg.startswith("-"):
            if arg != "-l":
                raise NotImplementedError("wc %s is not implemented" % arg)
        else:
            linecount += len(open(arg).read().splitlines())
    return "%s" % linecount


globals()["["] = test


# grep


def grep(pattern, *paths, **kwargs):
    import re

    pattern = re.compile(pattern)
    stdin = kwargs.get("stdin", "")
    for path in paths:
        stdin += open(path).read()
    return "".join(l for l in stdin.splitlines(True) if pattern.search(l))


# shell builtin


def source(path):
    name = os.path.basename(path)
    if name in {"helpers-usechg.sh", "histedit-helpers.sh"}:
        return

    defs = {}
    if name == "library.sh":
        name = os.path.basename(os.path.dirname(path))
        if name == "tests":
            # remotefilelog helpers
            from . import remotefilelog

            defs = remotefilelog.__dict__
        elif name == "hgsql":
            # hgsql helpers
            from . import hgsql

            defs = hgsql.__dict__
    if defs:
        for name, body in defs.items():
            if callable(body):
                globals()[name] = body
        return

    raise NotImplementedError("source not implemented")


globals()["."] = source


# hg commands


def hg(*args, **kwargs):
    (status, buf) = _hg(*args, **kwargs)
    if status:
        if not buf.endswith("\n") and buf:
            buf += "\n"
        buf += "[%s]" % status
    return buf


def hgexcept(*args, **kwargs):
    (status, buf) = _hg(*args, **kwargs)
    if status:
        raise RuntimeError("Exit code: {}. Output: {}".format(status, buf))
    return buf


def _hg(*args, **kwargs):
    stdin = kwargs.get("stdin") or ""
    encoding.setfromenviron()
    cwdbefore = os.getcwd()
    fout = util.stringio()
    fin = util.stringio(stdin)
    sysargs = ["hg"] + list(args)
    pycompat.sysargv = sysargs
    status = bindings.commands.run(sysargs, fin, fout, fout)
    cwdafter = os.getcwd()
    if cwdafter != cwdbefore:
        # Revert side effect of --cwd
        os.chdir(cwdbefore)
    buf = fout.getvalue().rstrip()
    return (status, buf)


# utilities in tinit.sh


def enable(*args):
    setconfig(*["extensions.%s=" % arg for arg in args])


def setconfig(*args):
    if os.path.exists(".hg"):
        hgrcpath = ".hg/hgrc"
    else:
        from .. import testtmp  # avoid side effect

        assert testtmp.HGRCPATH, "setconfig called before setuptesttmp"
        hgrcpath = testtmp.HGRCPATH
    content = ""
    for config in args:
        section, namevalue = config.split(".", 1)
        content = "\n[%s]\n%s\n" % (section, namevalue)
        util.appendfile(hgrcpath, content)


def setmodernconfig():
    enable("remotenames", "amend")
    setconfig(
        "experimental.narrow-heads=true",
        "visibility.enabled=true",
        "mutation.record=true",
        "mutation.enabled=true",
        "mutation.date=0 0",
        "experimental.evolution=obsolete",
        "remotenames.rename.default=remote",
    )


_newrepoid = 0


def newrepo(name=None):
    from .. import testtmp  # avoid side effect

    if name is None:
        global _newrepoid
        _newrepoid += 1
        name = "repo%s" % _newrepoid
    path = os.path.join(testtmp.TESTTMP, name)
    hg("init", path)
    cd(path)


def drawdag(*args, **kwargs):
    result = hg("debugdrawdag", *args, **kwargs)
    for line in hg("book", "-T", "{bookmark}={node}\n").splitlines():
        name, value = line.split("=", 1)
        os.environ[name] = value
        hg("book", "-d", name)
    return result


def showgraph():
    return hg("log", "-G", "-T", "{rev} {node|short} {desc|firstline}")


def tglog(*args):
    return hg(
        "log", "-G", "-T", "{rev}: {node|short} '{desc}' {bookmarks} {branches}", *args
    )


def tglogp(*args):
    return hg(
        "log",
        "-G",
        "-T",
        "{rev}: {node|short} {phase} '{desc}' {bookmarks} {branches}",
        *args
    )


def tglogm(*args):
    return hg(
        "log",
        "-G",
        "-T",
        "{rev}: {node|short} '{desc|firstline}' {bookmarks} {join(mutations % '(Rewritten using {operation} into {join(successors % '{node|short}', ', ')})', ' ')}",
        *args
    )


# helper specific to shlib


def expandpath(path):
    # Replace `pwd`, it is commonly used in tests.
    pwd = os.getcwd()
    path = path.replace("`pwd`", pwd).replace("$PWD", pwd)
    return util.expandpath(path)


def _lines(*args, **kwargs):
    """Shared logic for head and tail"""
    n = None
    content = ""
    for arg in args:
        if arg.startswith("-"):
            n = int(arg[1:])
        else:
            content += cat(arg)
    stdin = kwargs.get("stdin")
    if stdin is not None:
        content += stdin
    assert n is not None
    return n, content.splitlines(True)
