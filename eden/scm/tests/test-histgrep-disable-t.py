# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Simulate an environment that disables allowfullrepogrep:
sh % "setconfig 'histgrep.allowfullrepogrep=False'"

# Test histgrep and check that it respects the specified file:
sh % "hg init repo"
sh % "cd repo"
sh % "mkdir histgrepdir"
sh % "cd histgrepdir"
sh % "echo ababagalamaga" > "histgrepfile1"
sh % "echo ababagalamaga" > "histgrepfile2"
sh % "hg add histgrepfile1"
sh % "hg add histgrepfile2"
sh % "hg commit -m 'Added some files'"
sh % "hg histgrep ababagalamaga histgrepfile1" == "histgrepdir/histgrepfile1:0:ababagalamaga"
sh % "hg histgrep ababagalamaga" == r"""
    abort: can't run histgrep on the whole repo, please provide filenames
    (this is disabled to avoid very slow greps over the whole repo)
    [255]"""

# Now allow allowfullrepogrep:
sh % "setconfig 'histgrep.allowfullrepogrep=True'"
sh % "hg histgrep ababagalamaga" == r"""
    histgrepdir/histgrepfile1:0:ababagalamaga
    histgrepdir/histgrepfile2:0:ababagalamaga"""
sh % "cd .."
