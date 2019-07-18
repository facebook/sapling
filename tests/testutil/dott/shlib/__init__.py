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


# Use "edenscm" for shell utilities and main hg commands.
# Try to make "edenscm" available. Assuming sys.path includes "TESTDIR".
for candidate in sys.path:
    if os.path.exists(os.path.join(candidate, "../edenscm/__init__.py")):
        sys.path.insert(0, os.path.dirname(candidate))
        break
for candidate in sys.path:
    if os.path.exists(os.path.join(candidate, "hghave.py")):
        TESTDIR = os.path.abspath(candidate)
        break
else:
    raise RuntimeError("Cannot find TESTDIR")


try:
    from edenscm.mercurial import dispatch, ui as uimod, util
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
            if not result:
                return "[1]"
    raise NotImplementedError("test not fully implemented")


def touch(*args):
    for path in args:
        open(path, "a")


def true():
    return ""


globals()["["] = test


# hg commands


def hg(*args, **kwargs):
    stdin = kwargs.get("stdin")
    ui = uimod.ui()
    if "HGRCPATH" in os.environ:
        ui.readconfig(os.environ["HGRCPATH"])
    fout = util.stringio()
    req = dispatch.request(
        list(args), ui=ui, fin=util.stringio(stdin or ""), fout=fout, ferr=fout
    )
    status = (dispatch.dispatch(req) or 0) & 255
    buf = fout.getvalue().rstrip()
    if status:
        if not buf.endswith("\n") and buf:
            buf += "\n"
        buf += "[%s]" % status
    return buf


# helper specific to shlib


def reload(*args):
    """Reload edenscm Python modules. Cancel side effects by hg extensions."""
    todel = [k for k in sys.modules if k.startswith("edenscm")]
    for name in todel:
        del sys.modules[name]
    from edenscm.mercurial import dispatch, ui as uimod, util

    globals()["dispatch"] = dispatch
    globals()["uimod"] = uimod
    globals()["util"] = util


def expandpath(path):
    # Replace `pwd`, it is commonly used in tests.
    pwd = os.getcwd()
    path = path.replace("`pwd`", pwd).replace("$PWD", pwd)
    return util.expandpath(path)
