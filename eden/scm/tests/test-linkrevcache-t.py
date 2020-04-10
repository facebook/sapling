# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
linkrevcache=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"
sh % "touch a"
sh % "hg ci -A a -m a"
sh % "echo 1" >> "a"
sh % "hg ci -A a -m a1"
sh % "hg up '.^' -q"
sh % "hg graft --log 1 -q"
sh % "hg log -G -T '{rev}:{node} {desc}\\n'" == r"""
    @  2:e048e956c6a8c0f6108497df043989578ad97cc2 a1
    |  (grafted from da7a5140a61110d9ec1a678a11e796a71638dd6f)
    | o  1:da7a5140a61110d9ec1a678a11e796a71638dd6f a1
    |/
    o  0:3903775176ed42b1458a6281db4a0ccf4d9f287a a"""
sh % "hg debugbuildlinkrevcache --debug" == "a@d0c79e1d33097a72f79cb2e5a81c685e8f688d45: new linkrev 2"
sh % "hg debugverifylinkrevcache" == "1 entries verified"

sh % "hg annotate a -r 1" == "1: 1"
sh % "hg annotate a -r 2" == "2: 1"
