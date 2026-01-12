#!sl debugpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import re
import sys
from pathlib import Path

import sapling.testing.ext.mononoke as monotest
from sapling.testing.sh.bufio import BufIO
from sapling.testing.sh.interp import interpcode
from sapling.testing.sh.osfs import OSFS
from sapling.testing.sh.types import Env
from sapling.testing.t.runtime import TestTmp
from sapling.util import shellquote


class LegacyTestTmp(TestTmp):
    def __exit__(self, et, ev, tb):
        pass

    def _setup(self, _tmpprefix):
        existing_testtmp = os.getenv("TESTTMP")
        path = Path(os.path.realpath(existing_testtmp))
        fs = OSFS()
        fs.chdir(os.getcwd())
        envvars = os.environ
        shenv = Env(
            fs=fs,
            envvars=envvars,
            exportedenvvars=set(envvars),
            cmdtable=self._initialshellcmdtable(),
            stdin=BufIO(),
        )
        pyenv = {
            "atexit": self.atexit,
            "checkoutput": self.checkoutput,
            "command": self.command,
            "getenv": self.getenv,
            "hasfeature": self.hasfeature,
            "pydoceval": self.pydoceval,
            "requireexe": self.requireexe,
            "require": self.require,
            "setenv": self.setenv,
            "sheval": self.sheval,
            "TESTTMP": path,
        }
        self.path = path
        self.should_delete_path = not existing_testtmp
        self.shenv = shenv
        self.pyenv = pyenv
        self.substitutions = [(re.escape(str(path)), "$TESTTMP")]


t = LegacyTestTmp()
origenvs = dict(t.shenv.envvars)

monotest.setupfuncs(t)

qargs = [shellquote(a) for a in sys.argv[1:]]
res = interpcode(" ".join(qargs), t.shenv)
print(res.out, end="")

with open(os.environ.get("TESTTMP") / Path(".dbrtest_envs"), "w") as f:
    for k, v in t.shenv.envvars.items():
        if k == "PWD" or origenvs.get(k) == v:
            continue
        if len(v) >= 2 and v[0] == "(" and v[-1] == ")":
            quoted = "(" + " ".join([shellquote(a) for a in v[1:-1].split(" ")]) + ")"
        else:
            quoted = shellquote(v)
        f.write(f"export {k}={quoted}\n")

sys.exit(res.exitcode)
