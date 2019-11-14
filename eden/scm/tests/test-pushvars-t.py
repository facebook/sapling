# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

# Setup

sh % "cat" << r"""
#!/bin/bash
env | egrep "^HG_USERVAR_(DEBUG|BYPASS_REVIEW)" | sort
exit 0
""" > "$TESTTMP/pretxnchangegroup.sh"
sh % "cat" << r"""
[hooks]
pretxnchangegroup = bash $TESTTMP/pretxnchangegroup.sh
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "hg clone -q repo child"
sh % "cd child"

# Test pushing vars to repo with pushvars.server explicitly disabled

sh % "cd ../repo"
sh % "setconfig 'push.pushvars.server=False'"
sh % "cd ../child"
sh % "echo b" > "a"
sh % "hg commit -Aqm a"
sh % "hg push --pushvars 'DEBUG=1' --pushvars 'BYPASS_REVIEW=true' --config 'push.pushvars.server=False'" == r"""
    pushing to $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files"""

# Setting pushvars.sever = true and then pushing.

sh % "cd ../repo"
sh % "setconfig 'push.pushvars.server=True'"
sh % "cd ../child"
sh % "echo b" >> "a"
sh % "hg commit -Aqm a"
sh % "hg push --pushvars 'DEBUG=1' --pushvars 'BYPASS_REVIEW=true'" == r"""
    pushing to $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    HG_USERVAR_BYPASS_REVIEW=true
    HG_USERVAR_DEBUG=1"""

# Test pushing var with empty right-hand side

sh % "echo b" >> "a"
sh % "hg commit -Aqm a"
sh % "hg push --pushvars 'DEBUG='" == r"""
    pushing to $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    HG_USERVAR_DEBUG="""

# Test pushing bad vars

sh % "echo b" >> "a"
sh % "hg commit -Aqm b"
sh % "hg push --pushvars DEBUG" == r"""
    pushing to $TESTTMP/repo
    searching for changes
    abort: unable to parse variable 'DEBUG', should follow 'KEY=VALUE' or 'KEY=' format
    [255]"""

# Test Python hooks

sh % "cat" << r"""
def hook(ui, repo, hooktype, **kwargs):
    for k, v in sorted(kwargs.items()):
        if "USERVAR" in k:
            ui.write("Got pushvar: %s=%s\n" % (k, v))
""" >> "$TESTTMP/pyhook.py"

sh % 'cp "$HGRCPATH" "$TESTTMP/hgrc.bak"'
sh % "cat" << r"""
[hooks]
pretxnchangegroup.pyhook = python:$TESTTMP/pyhook.py:hook
""" >> "$HGRCPATH"

sh % "hg push --pushvars 'A=1' --pushvars 'B=2'" == r"""
    pushing to $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    Got pushvar: USERVAR_A=1
    Got pushvar: USERVAR_B=2"""
sh % 'cp "$TESTTMP/hgrc.bak" "$HGRCPATH"'

# Test pushvars for enforcing push reasons
sh % "cat" << r"""
[push]
requirereason=True
requirereasonmsg="Because I said so"
""" >> ".hg/hgrc"
sh % "echo c" >> "a"
sh % "hg commit -Aqm c"
sh % "hg push" == r"""
    pushing to $TESTTMP/repo
    abort: "Because I said so"
    (use `--pushvars PUSH_REASON='because ...'`)
    [255]"""
sh % "hg push --pushvars 'PUSH_REASON=I want to'" == r"""
    pushing to $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files"""
sh % 'hg blackbox --pattern \'{"legacy_log": {"service": "pushreason"}}\'' == "* [legacy][pushreason] bypassing push block with reason: I want to (glob)"
sh % 'cp "$TESTTMP/hgrc.bak" "$HGRCPATH"'
