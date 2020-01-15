# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"

# This test file test the various templates related to obsmarkers.

# Global setup
# ============

sh % "cat" << r"""
[ui]
interactive = true
[phases]
publish=False
[experimental]
evolution=true
[templates]
obsfatesuccessors = "{if(successors, " as ")}{join(successors, ", ")}"
obsfateverb = "{obsfateverb(successors, markers)}"
obsfateoperations = "{if(obsfateoperations(markers), " using {join(obsfateoperations(markers), ", ")}")}"
obsfateusers = "{if(obsfateusers(markers), " by {join(obsfateusers(markers), ", ")}")}"
obsfatedate = "{if(obsfatedate(markers), "{ifeq(min(obsfatedate(markers)), max(obsfatedate(markers)), " (at {min(obsfatedate(markers))|isodate})", " (between {min(obsfatedate(markers))|isodate} and {max(obsfatedate(markers))|isodate})")}")}"
obsfatetempl = "{obsfateverb}{obsfateoperations}{obsfatesuccessors}{obsfateusers}{obsfatedate}; "
[alias]
tlog = log -G -T '{node|short}    {if(predecessors, "\n  Predecessors: {predecessors}")}    {if(predecessors, "\n  semi-colon: {join(predecessors, "; ")}")}    {if(predecessors, "\n  json: {predecessors|json}")}    {if(predecessors, "\n  map: {join(predecessors % "{rev}:{node}", " ")}")}    {if(successorssets, "\n  Successors: {successorssets}")}    {if(successorssets, "\n  multi-line: {join(successorssets, "\n  multi-line: ")}")}    {if(successorssets, "\n  json: {successorssets|json}")}\n'
fatelog = log -G -T '{node|short}\n{if(succsandmarkers, "  Obsfate: {succsandmarkers % "{obsfatetempl}"} \n" )}'
fatelogjson = log -G -T '{node|short}\n{if(succsandmarkers, "  Obsfate: {succsandmarkers|json}\n")}'
fatelogkw = log -G -T '{node|short}\n{if(obsfate, "{obsfate % "  Obsfate: {fate}\n"}")}'
fatelogcount = log -G -T '{node|short} {succsandmarkers}'
""" >> "$HGRCPATH"


def mkcommit(name):
    open(name, "wb").write("%s\n" % name)
    sh.hg("commit", "-m%s" % name, "-A", name)


shlib.mkcommit = mkcommit

# Test templates on amended commit
# ================================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/templates-local-amend"'
sh % 'cd "$TESTTMP/templates-local-amend"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % "echo 42" >> "A0"
sh % "hg commit --amend -m A1 --config 'devel.default-date=1234567890 0'"
sh % "hg commit --amend -m A2 --config 'devel.default-date=987654321 0' --config 'devel.user.obsmarker=test2'"

sh % "hg log --hidden -G" == r"""
    @  changeset:   3:d004c8f274b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | x  changeset:   2:a468dc9b3633
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:d004c8f274b9 by test2
    |    summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:a468dc9b3633
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Check templates
# ---------------
sh % "hg up 'desc(A0)' --hidden" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  d004c8f274b9
    |    Predecessors: 1:471f378eab4c
    |    semi-colon: 1:471f378eab4c
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  471f378eab4c
    |/     Successors: 3:d004c8f274b9
    |      multi-line: 3:d004c8f274b9
    |      json: [["d004c8f274b9ec480a47a93c10dac5eee63adb78"]]
    o  ea207398892e"""
sh % "hg fatelog" == r"""
    o  d004c8f274b9
    |
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test, test2 (between 2009-02-13 23:31 +0000 and 2009-02-13 23:31 +0000);
    o  ea207398892e"""

sh % "hg fatelogkw" == r"""
    o  d004c8f274b9
    |
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test, test2
    o  ea207398892e"""

sh % "hg log -G --config 'ui.logtemplate='" == r"""
    o  changeset:   3:d004c8f274b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | @  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:d004c8f274b9 by test, test2
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

sh % "hg log -G -T default" == r"""
    o  changeset:   3:d004c8f274b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | @  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:d004c8f274b9 by test, test2
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % "hg up 'desc(A1)' --hidden" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  d004c8f274b9
    |    Predecessors: 2:a468dc9b3633
    |    semi-colon: 2:a468dc9b3633
    |    json: ["a468dc9b36338b14fdb7825f55ce3df4e71517ad"]
    |    map: 2:a468dc9b36338b14fdb7825f55ce3df4e71517ad
    | @  a468dc9b3633
    |/     Successors: 3:d004c8f274b9
    |      multi-line: 3:d004c8f274b9
    |      json: [["d004c8f274b9ec480a47a93c10dac5eee63adb78"]]
    o  ea207398892e"""
sh % "hg fatelog" == r"""
    o  d004c8f274b9
    |
    | @  a468dc9b3633
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test2 (at 2009-02-13 23:31 +0000);
    o  ea207398892e"""
# Predecessors template should show all the predecessors as we force their display
# with --hidden
sh % "hg tlog --hidden" == r"""
    o  d004c8f274b9
    |    Predecessors: 2:a468dc9b3633
    |    semi-colon: 2:a468dc9b3633
    |    json: ["a468dc9b36338b14fdb7825f55ce3df4e71517ad"]
    |    map: 2:a468dc9b36338b14fdb7825f55ce3df4e71517ad
    | @  a468dc9b3633
    |/     Predecessors: 1:471f378eab4c
    |      semi-colon: 1:471f378eab4c
    |      json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |      map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    |      Successors: 3:d004c8f274b9
    |      multi-line: 3:d004c8f274b9
    |      json: [["d004c8f274b9ec480a47a93c10dac5eee63adb78"]]
    | x  471f378eab4c
    |/     Successors: 2:a468dc9b3633
    |      multi-line: 2:a468dc9b3633
    |      json: [["a468dc9b36338b14fdb7825f55ce3df4e71517ad"]]
    o  ea207398892e"""
sh % "hg fatelog --hidden" == r"""
    o  d004c8f274b9
    |
    | @  a468dc9b3633
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test2 (at 2009-02-13 23:31 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:a468dc9b3633 by test (at 2009-02-13 23:31 +0000);
    o  ea207398892e"""

# Predecessors template shouldn't show anything as all obsolete commit are not
# visible.
sh % "hg up 'desc(A2)'" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg tlog" == r"""
    @  d004c8f274b9
    |
    o  ea207398892e"""
sh % "hg tlog --hidden" == r"""
    @  d004c8f274b9
    |    Predecessors: 2:a468dc9b3633
    |    semi-colon: 2:a468dc9b3633
    |    json: ["a468dc9b36338b14fdb7825f55ce3df4e71517ad"]
    |    map: 2:a468dc9b36338b14fdb7825f55ce3df4e71517ad
    | x  a468dc9b3633
    |/     Predecessors: 1:471f378eab4c
    |      semi-colon: 1:471f378eab4c
    |      json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |      map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    |      Successors: 3:d004c8f274b9
    |      multi-line: 3:d004c8f274b9
    |      json: [["d004c8f274b9ec480a47a93c10dac5eee63adb78"]]
    | x  471f378eab4c
    |/     Successors: 2:a468dc9b3633
    |      multi-line: 2:a468dc9b3633
    |      json: [["a468dc9b36338b14fdb7825f55ce3df4e71517ad"]]
    o  ea207398892e"""
sh % "hg fatelog" == r"""
    @  d004c8f274b9
    |
    o  ea207398892e"""

sh % "hg fatelog --hidden" == r"""
    @  d004c8f274b9
    |
    | x  a468dc9b3633
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test2 (at 2009-02-13 23:31 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:a468dc9b3633 by test (at 2009-02-13 23:31 +0000);
    o  ea207398892e"""
sh % "hg fatelogjson --hidden" == r"""
    @  d004c8f274b9
    |
    | x  a468dc9b3633
    |/     Obsfate: [{"markers": [["a468dc9b36338b14fdb7825f55ce3df4e71517ad", ["d004c8f274b9ec480a47a93c10dac5eee63adb78"], 0, [["operation", "amend"], ["user", "test2"]], [1234567891.0, 0], null]], "successors": ["d004c8f274b9ec480a47a93c10dac5eee63adb78"]}]
    | x  471f378eab4c
    |/     Obsfate: [{"markers": [["471f378eab4c5e25f6c77f785b27c936efb22874", ["a468dc9b36338b14fdb7825f55ce3df4e71517ad"], 0, [["operation", "amend"], ["user", "test"]], [1234567890.0, 0], null]], "successors": ["a468dc9b36338b14fdb7825f55ce3df4e71517ad"]}]
    o  ea207398892e"""

# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    @  d004c8f274b9
    |
    | x  a468dc9b3633
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:a468dc9b3633
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    @  d004c8f274b9
    |
    | x  a468dc9b3633
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test2
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:a468dc9b3633
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    @  d004c8f274b9
    |
    | x  a468dc9b3633
    |/     Obsfate: rewritten using amend as 3:d004c8f274b9 by test2 (at 2009-02-13 23:31 +0000)
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:a468dc9b3633 by test (at 2009-02-13 23:31 +0000)
    o  ea207398892e"""

sh % "hg log -G -T default --hidden" == r"""
    @  changeset:   3:d004c8f274b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | x  changeset:   2:a468dc9b3633
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:d004c8f274b9 by test2
    |    summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:a468dc9b3633
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % "hg log -G -T default --hidden -v" == r"""
    @  changeset:   3:d004c8f274b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  files:       A0
    |  description:
    |  A2
    |
    |
    | x  changeset:   2:a468dc9b3633
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:d004c8f274b9 by test2 (at 2009-02-13 23:31 +0000)
    |    files:       A0
    |    description:
    |    A1
    |
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:a468dc9b3633 by test (at 2009-02-13 23:31 +0000)
    |    files:       A0
    |    description:
    |    A0
    |
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       files:       ROOT
       description:
       ROOT"""
# Test templates with splitted commit
# ===================================

sh % 'hg init "$TESTTMP/templates-local-split"'
sh % 'cd "$TESTTMP/templates-local-split"'
sh % "mkcommit ROOT"
sh % "echo 42" >> "a"
sh % "echo 43" >> "b"
sh % "hg commit -A -m A0" == r"""
    adding a
    adding b"""
sh % "hg log --hidden -G" == r"""
    @  changeset:   1:471597cad322
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Simulate split
sh % "hg up -r 'desc(ROOT)'" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo 42" >> "a"
sh % "hg commit -A -m A0" == "adding a"
sh % "echo 43" >> "b"
sh % "hg commit -A -m A0" == "adding b"
sh % "hg debugobsolete 1 2 3" == "obsoleted 1 changesets"

sh % "hg log --hidden -G" == r"""
    @  changeset:   3:f257fde29c7a
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   2:337fec4d2edc
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    | x  changeset:   1:471597cad322
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    split as 2:337fec4d2edc, 3:f257fde29c7a
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Check templates
# ---------------

sh % "hg up 'obsolete()' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  f257fde29c7a
    |    Predecessors: 1:471597cad322
    |    semi-colon: 1:471597cad322
    |    json: ["471597cad322d1f659bb169751be9133dad92ef3"]
    |    map: 1:471597cad322d1f659bb169751be9133dad92ef3
    o  337fec4d2edc
    |    Predecessors: 1:471597cad322
    |    semi-colon: 1:471597cad322
    |    json: ["471597cad322d1f659bb169751be9133dad92ef3"]
    |    map: 1:471597cad322d1f659bb169751be9133dad92ef3
    | @  471597cad322
    |/     Successors: 2:337fec4d2edc 3:f257fde29c7a
    |      multi-line: 2:337fec4d2edc 3:f257fde29c7a
    |      json: [["337fec4d2edcf0e7a467e35f818234bc620068b5", "f257fde29c7a847c9b607f6e958656d0df0fb15c"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  f257fde29c7a
    |
    o  337fec4d2edc
    |
    | @  471597cad322
    |/     Obsfate: split as 2:337fec4d2edc, 3:f257fde29c7a by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up f257fde29c7a" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should not show a predecessor as it's not displayed in
# the log
sh % "hg tlog" == r"""
    @  f257fde29c7a
    |
    o  337fec4d2edc
    |
    o  ea207398892e"""
# Predecessors template should show both predecessors as we force their display
# with --hidden
sh % "hg tlog --hidden" == r"""
    @  f257fde29c7a
    |    Predecessors: 1:471597cad322
    |    semi-colon: 1:471597cad322
    |    json: ["471597cad322d1f659bb169751be9133dad92ef3"]
    |    map: 1:471597cad322d1f659bb169751be9133dad92ef3
    o  337fec4d2edc
    |    Predecessors: 1:471597cad322
    |    semi-colon: 1:471597cad322
    |    json: ["471597cad322d1f659bb169751be9133dad92ef3"]
    |    map: 1:471597cad322d1f659bb169751be9133dad92ef3
    | x  471597cad322
    |/     Successors: 2:337fec4d2edc 3:f257fde29c7a
    |      multi-line: 2:337fec4d2edc 3:f257fde29c7a
    |      json: [["337fec4d2edcf0e7a467e35f818234bc620068b5", "f257fde29c7a847c9b607f6e958656d0df0fb15c"]]
    o  ea207398892e"""

sh % "hg fatelog --hidden" == r"""
    @  f257fde29c7a
    |
    o  337fec4d2edc
    |
    | x  471597cad322
    |/     Obsfate: split as 2:337fec4d2edc, 3:f257fde29c7a by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg fatelogjson --hidden" == r"""
    @  f257fde29c7a
    |
    o  337fec4d2edc
    |
    | x  471597cad322
    |/     Obsfate: [{"markers": [["471597cad322d1f659bb169751be9133dad92ef3", ["337fec4d2edcf0e7a467e35f818234bc620068b5", "f257fde29c7a847c9b607f6e958656d0df0fb15c"], 0, [["user", "test"]], [0.0, 0], null]], "successors": ["337fec4d2edcf0e7a467e35f818234bc620068b5", "f257fde29c7a847c9b607f6e958656d0df0fb15c"]}]
    o  ea207398892e"""
# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    @  f257fde29c7a
    |
    o  337fec4d2edc
    |
    | x  471597cad322
    |/     Obsfate: split as 2:337fec4d2edc, 3:f257fde29c7a
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    @  f257fde29c7a
    |
    o  337fec4d2edc
    |
    | x  471597cad322
    |/     Obsfate: split as 2:337fec4d2edc, 3:f257fde29c7a
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    @  f257fde29c7a
    |
    o  337fec4d2edc
    |
    | x  471597cad322
    |/     Obsfate: split as 2:337fec4d2edc, 3:f257fde29c7a by test (at 1970-01-01 00:00 +0000)
    o  ea207398892e"""

sh % "hg log -G -T default --hidden" == r"""
    @  changeset:   3:f257fde29c7a
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   2:337fec4d2edc
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    | x  changeset:   1:471597cad322
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    split as 2:337fec4d2edc, 3:f257fde29c7a
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Test templates with folded commit
# =================================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/templates-local-fold"'
sh % 'cd "$TESTTMP/templates-local-fold"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % "mkcommit B0"
sh % "hg log --hidden -G" == r"""
    @  changeset:   2:0dec01379d3b
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     B0
    |
    o  changeset:   1:471f378eab4c
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Simulate a fold
sh % "hg up -r 'desc(ROOT)'" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo A0" > "A0"
sh % "echo B0" > "B0"
sh % "hg commit -A -m C0" == r"""
    adding A0
    adding B0"""
sh % "hg debugobsolete 'desc(A0)' 'desc(C0)'" == "obsoleted 1 changesets"
sh % "hg debugobsolete 'desc(B0)' 'desc(C0)'" == "obsoleted 1 changesets"

sh % "hg log --hidden -G" == r"""
    @  changeset:   3:eb5a0daa2192
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     C0
    |
    | x  changeset:   2:0dec01379d3b
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  obsolete:    rewritten as 3:eb5a0daa2192
    | |  summary:     B0
    | |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten as 3:eb5a0daa2192
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Check templates
# ---------------

sh % "hg up 'desc(A0)' --hidden" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  eb5a0daa2192
    |    Predecessors: 1:471f378eab4c
    |    semi-colon: 1:471f378eab4c
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  471f378eab4c
    |/     Successors: 3:eb5a0daa2192
    |      multi-line: 3:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  eb5a0daa2192
    |
    | @  471f378eab4c
    |/     Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(B0)' --hidden" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show both predecessors as they should be both
# displayed
sh % "hg tlog" == r"""
    o  eb5a0daa2192
    |    Predecessors: 2:0dec01379d3b 1:471f378eab4c
    |    semi-colon: 2:0dec01379d3b; 1:471f378eab4c
    |    json: ["0dec01379d3be6318c470ead31b1fe7ae7cb53d5", "471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 2:0dec01379d3be6318c470ead31b1fe7ae7cb53d5 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  0dec01379d3b
    | |    Successors: 3:eb5a0daa2192
    | |    multi-line: 3:eb5a0daa2192
    | |    json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    | x  471f378eab4c
    |/     Successors: 3:eb5a0daa2192
    |      multi-line: 3:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  eb5a0daa2192
    |
    | @  0dec01379d3b
    | |    Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(C0)'" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should not show predecessors as they are not displayed in
# the log
sh % "hg tlog" == r"""
    @  eb5a0daa2192
    |
    o  ea207398892e"""
# Predecessors template should show both predecessors as we force their display
# with --hidden
sh % "hg tlog --hidden" == r"""
    @  eb5a0daa2192
    |    Predecessors: 2:0dec01379d3b 1:471f378eab4c
    |    semi-colon: 2:0dec01379d3b; 1:471f378eab4c
    |    json: ["0dec01379d3be6318c470ead31b1fe7ae7cb53d5", "471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 2:0dec01379d3be6318c470ead31b1fe7ae7cb53d5 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | x  0dec01379d3b
    | |    Successors: 3:eb5a0daa2192
    | |    multi-line: 3:eb5a0daa2192
    | |    json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    | x  471f378eab4c
    |/     Successors: 3:eb5a0daa2192
    |      multi-line: 3:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog --hidden" == r"""
    @  eb5a0daa2192
    |
    | x  0dec01379d3b
    | |    Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""

sh % "hg fatelogjson --hidden" == r"""
    @  eb5a0daa2192
    |
    | x  0dec01379d3b
    | |    Obsfate: [{"markers": [["0dec01379d3be6318c470ead31b1fe7ae7cb53d5", ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"], 0, [["user", "test"]], [0.0, 0], null]], "successors": ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]}]
    | x  471f378eab4c
    |/     Obsfate: [{"markers": [["471f378eab4c5e25f6c77f785b27c936efb22874", ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"], 0, [["user", "test"]], [0.0, 0], null]], "successors": ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]}]
    o  ea207398892e"""
# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    @  eb5a0daa2192
    |
    | x  0dec01379d3b
    | |    Obsfate: rewritten as 3:eb5a0daa2192
    | x  471f378eab4c
    |/     Obsfate: rewritten as 3:eb5a0daa2192
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    @  eb5a0daa2192
    |
    | x  0dec01379d3b
    | |    Obsfate: rewritten as 3:eb5a0daa2192
    | x  471f378eab4c
    |/     Obsfate: rewritten as 3:eb5a0daa2192
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    @  eb5a0daa2192
    |
    | x  0dec01379d3b
    | |    Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000)
    | x  471f378eab4c
    |/     Obsfate: rewritten as 3:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000)
    o  ea207398892e"""
sh % "hg log -G -T default --hidden" == r"""
    @  changeset:   3:eb5a0daa2192
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     C0
    |
    | x  changeset:   2:0dec01379d3b
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  obsolete:    rewritten as 3:eb5a0daa2192
    | |  summary:     B0
    | |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten as 3:eb5a0daa2192
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Test templates with divergence
# ==============================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/templates-local-divergence"'
sh % 'cd "$TESTTMP/templates-local-divergence"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % "hg commit --amend -m A1"
sh % "hg log --hidden -G" == r"""
    @  changeset:   2:fdf9bde5129a
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:fdf9bde5129a
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % "hg update --hidden 'desc(A0)'" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg commit --amend -m A2"
sh % "hg log --hidden -G" == r"""
    @  changeset:   3:65b757b745b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  instability: content-divergent
    |  summary:     A2
    |
    | o  changeset:   2:fdf9bde5129a
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    instability: content-divergent
    |    summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:fdf9bde5129a
    |    obsolete:    rewritten using amend as 3:65b757b745b9
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % "hg commit --amend -m A3"
sh % "hg log --hidden -G" == r"""
    @  changeset:   4:019fadeab383
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  instability: content-divergent
    |  summary:     A3
    |
    | x  changeset:   3:65b757b745b9
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 4:019fadeab383
    |    summary:     A2
    |
    | o  changeset:   2:fdf9bde5129a
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    instability: content-divergent
    |    summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:fdf9bde5129a
    |    obsolete:    rewritten using amend as 3:65b757b745b9
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Check templates
# ---------------

sh % "hg up 'desc(A0)' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  019fadeab383
    |    Predecessors: 1:471f378eab4c
    |    semi-colon: 1:471f378eab4c
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | o  fdf9bde5129a
    |/     Predecessors: 1:471f378eab4c
    |      semi-colon: 1:471f378eab4c
    |      json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |      map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  471f378eab4c
    |/     Successors: 2:fdf9bde5129a; 4:019fadeab383
    |      multi-line: 2:fdf9bde5129a
    |      multi-line: 4:019fadeab383
    |      json: [["fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e"], ["019fadeab383f6699fa83ad7bdb4d82ed2c0e5ab"]]
    o  ea207398892e"""
sh % "hg fatelog" == r"""
    o  019fadeab383
    |
    | o  fdf9bde5129a
    |/
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a by test (at 1970-01-01 00:00 +0000); rewritten using amend as 4:019fadeab383 by test (between 1970-01-01 00:00 +0000 and 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(A1)'" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should not show predecessors as they are not displayed in
# the log
sh % "hg tlog" == r"""
    o  019fadeab383
    |
    | @  fdf9bde5129a
    |/
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  019fadeab383
    |
    | @  fdf9bde5129a
    |/
    o  ea207398892e"""
# Predecessors template should the predecessors as we force their display with
# --hidden
sh % "hg tlog --hidden" == r"""
    o  019fadeab383
    |    Predecessors: 3:65b757b745b9
    |    semi-colon: 3:65b757b745b9
    |    json: ["65b757b745b935093c87a2bccd877521cccffcbd"]
    |    map: 3:65b757b745b935093c87a2bccd877521cccffcbd
    | x  65b757b745b9
    |/     Predecessors: 1:471f378eab4c
    |      semi-colon: 1:471f378eab4c
    |      json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |      map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    |      Successors: 4:019fadeab383
    |      multi-line: 4:019fadeab383
    |      json: [["019fadeab383f6699fa83ad7bdb4d82ed2c0e5ab"]]
    | @  fdf9bde5129a
    |/     Predecessors: 1:471f378eab4c
    |      semi-colon: 1:471f378eab4c
    |      json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |      map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | x  471f378eab4c
    |/     Successors: 2:fdf9bde5129a; 3:65b757b745b9
    |      multi-line: 2:fdf9bde5129a
    |      multi-line: 3:65b757b745b9
    |      json: [["fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e"], ["65b757b745b935093c87a2bccd877521cccffcbd"]]
    o  ea207398892e"""

sh % "hg fatelog --hidden" == r"""
    o  019fadeab383
    |
    | x  65b757b745b9
    |/     Obsfate: rewritten using amend as 4:019fadeab383 by test (at 1970-01-01 00:00 +0000);
    | @  fdf9bde5129a
    |/
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a by test (at 1970-01-01 00:00 +0000); rewritten using amend as 3:65b757b745b9 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""

sh % "hg fatelogjson --hidden" == r"""
    o  019fadeab383
    |
    | x  65b757b745b9
    |/     Obsfate: [{"markers": [["65b757b745b935093c87a2bccd877521cccffcbd", ["019fadeab383f6699fa83ad7bdb4d82ed2c0e5ab"], 0, [["operation", "amend"], ["user", "test"]], [1.0, 0], null]], "successors": ["019fadeab383f6699fa83ad7bdb4d82ed2c0e5ab"]}]
    | @  fdf9bde5129a
    |/
    | x  471f378eab4c
    |/     Obsfate: [{"markers": [["471f378eab4c5e25f6c77f785b27c936efb22874", ["fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e"], 0, [["operation", "amend"], ["user", "test"]], [0.0, 0], null]], "successors": ["fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e"]}, {"markers": [["471f378eab4c5e25f6c77f785b27c936efb22874", ["65b757b745b935093c87a2bccd877521cccffcbd"], 0, [["operation", "amend"], ["user", "test"]], [0.0, 0], null]], "successors": ["65b757b745b935093c87a2bccd877521cccffcbd"]}]
    o  ea207398892e"""

# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    o  019fadeab383
    |
    | x  65b757b745b9
    |/     Obsfate: rewritten using amend as 4:019fadeab383
    | @  fdf9bde5129a
    |/
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a
    |      Obsfate: rewritten using amend as 3:65b757b745b9
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    o  019fadeab383
    |
    | x  65b757b745b9
    |/     Obsfate: rewritten using amend as 4:019fadeab383
    | @  fdf9bde5129a
    |/
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a
    |      Obsfate: rewritten using amend as 3:65b757b745b9
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    o  019fadeab383
    |
    | x  65b757b745b9
    |/     Obsfate: rewritten using amend as 4:019fadeab383 by test (at 1970-01-01 00:00 +0000)
    | @  fdf9bde5129a
    |/
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a by test (at 1970-01-01 00:00 +0000)
    |      Obsfate: rewritten using amend as 3:65b757b745b9 by test (at 1970-01-01 00:00 +0000)
    o  ea207398892e"""
sh % "hg log -G -T default --hidden" == r"""
    o  changeset:   4:019fadeab383
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  instability: content-divergent
    |  summary:     A3
    |
    | x  changeset:   3:65b757b745b9
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 4:019fadeab383
    |    summary:     A2
    |
    | @  changeset:   2:fdf9bde5129a
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    instability: content-divergent
    |    summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:fdf9bde5129a
    |    obsolete:    rewritten using amend as 3:65b757b745b9
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Test templates with amended + folded commit
# ===========================================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/templates-local-amend-fold"'
sh % 'cd "$TESTTMP/templates-local-amend-fold"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % "mkcommit B0"
sh % "hg commit --amend -m B1"
sh % "hg log --hidden -G" == r"""
    @  changeset:   3:b7ea6d14e664
    |  parent:      1:471f378eab4c
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     B1
    |
    | x  changeset:   2:0dec01379d3b
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:b7ea6d14e664
    |    summary:     B0
    |
    o  changeset:   1:471f378eab4c
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Simulate a fold
sh % "hg up -r 'desc(ROOT)'" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo A0" > "A0"
sh % "echo B0" > "B0"
sh % "hg commit -A -m C0" == r"""
    adding A0
    adding B0"""
sh % "hg debugobsolete 'desc(A0)' 'desc(C0)'" == "obsoleted 1 changesets"
sh % "hg debugobsolete 'desc(B1)' 'desc(C0)'" == "obsoleted 1 changesets"

sh % "hg log --hidden -G" == r"""
    @  changeset:   4:eb5a0daa2192
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     C0
    |
    | x  changeset:   3:b7ea6d14e664
    | |  parent:      1:471f378eab4c
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  obsolete:    rewritten as 4:eb5a0daa2192
    | |  summary:     B1
    | |
    | | x  changeset:   2:0dec01379d3b
    | |/   user:        test
    | |    date:        Thu Jan 01 00:00:00 1970 +0000
    | |    obsolete:    rewritten using amend as 3:b7ea6d14e664
    | |    summary:     B0
    | |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten as 4:eb5a0daa2192
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Check templates
# ---------------

sh % "hg up 'desc(A0)' --hidden" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  eb5a0daa2192
    |    Predecessors: 1:471f378eab4c
    |    semi-colon: 1:471f378eab4c
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  471f378eab4c
    |/     Successors: 4:eb5a0daa2192
    |      multi-line: 4:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  eb5a0daa2192
    |
    | @  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(B0)' --hidden" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should both predecessors as they are visible
sh % "hg tlog" == r"""
    o  eb5a0daa2192
    |    Predecessors: 2:0dec01379d3b 1:471f378eab4c
    |    semi-colon: 2:0dec01379d3b; 1:471f378eab4c
    |    json: ["0dec01379d3be6318c470ead31b1fe7ae7cb53d5", "471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 2:0dec01379d3be6318c470ead31b1fe7ae7cb53d5 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  0dec01379d3b
    | |    Successors: 4:eb5a0daa2192
    | |    multi-line: 4:eb5a0daa2192
    | |    json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    | x  471f378eab4c
    |/     Successors: 4:eb5a0daa2192
    |      multi-line: 4:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  eb5a0daa2192
    |
    | @  0dec01379d3b
    | |    Obsfate: rewritten using amend as 4:eb5a0daa2192 by test (between 1970-01-01 00:00 +0000 and 1970-01-01 00:00 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(B1)' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should both predecessors as they are visible
sh % "hg tlog" == r"""
    o  eb5a0daa2192
    |    Predecessors: 1:471f378eab4c 3:b7ea6d14e664
    |    semi-colon: 1:471f378eab4c; 3:b7ea6d14e664
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874", "b7ea6d14e664bdc8922221f7992631b50da3fb07"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874 3:b7ea6d14e664bdc8922221f7992631b50da3fb07
    | @  b7ea6d14e664
    | |    Successors: 4:eb5a0daa2192
    | |    multi-line: 4:eb5a0daa2192
    | |    json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    | x  471f378eab4c
    |/     Successors: 4:eb5a0daa2192
    |      multi-line: 4:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  eb5a0daa2192
    |
    | @  b7ea6d14e664
    | |    Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(C0)'" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show no predecessors as they are both non visible
sh % "hg tlog" == r"""
    @  eb5a0daa2192
    |
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    @  eb5a0daa2192
    |
    o  ea207398892e"""
# Predecessors template should show all predecessors as we force their display
# with --hidden
sh % "hg tlog --hidden" == r"""
    @  eb5a0daa2192
    |    Predecessors: 1:471f378eab4c 3:b7ea6d14e664
    |    semi-colon: 1:471f378eab4c; 3:b7ea6d14e664
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874", "b7ea6d14e664bdc8922221f7992631b50da3fb07"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874 3:b7ea6d14e664bdc8922221f7992631b50da3fb07
    | x  b7ea6d14e664
    | |    Predecessors: 2:0dec01379d3b
    | |    semi-colon: 2:0dec01379d3b
    | |    json: ["0dec01379d3be6318c470ead31b1fe7ae7cb53d5"]
    | |    map: 2:0dec01379d3be6318c470ead31b1fe7ae7cb53d5
    | |    Successors: 4:eb5a0daa2192
    | |    multi-line: 4:eb5a0daa2192
    | |    json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    | | x  0dec01379d3b
    | |/     Successors: 3:b7ea6d14e664
    | |      multi-line: 3:b7ea6d14e664
    | |      json: [["b7ea6d14e664bdc8922221f7992631b50da3fb07"]]
    | x  471f378eab4c
    |/     Successors: 4:eb5a0daa2192
    |      multi-line: 4:eb5a0daa2192
    |      json: [["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]]
    o  ea207398892e"""

sh % "hg fatelog --hidden" == r"""
    @  eb5a0daa2192
    |
    | x  b7ea6d14e664
    | |    Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    | | x  0dec01379d3b
    | |/     Obsfate: rewritten using amend as 3:b7ea6d14e664 by test (at 1970-01-01 00:00 +0000);
    | x  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""

sh % "hg fatelogjson --hidden" == r"""
    @  eb5a0daa2192
    |
    | x  b7ea6d14e664
    | |    Obsfate: [{"markers": [["b7ea6d14e664bdc8922221f7992631b50da3fb07", ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"], 0, [["user", "test"]], [1.0, 0], null]], "successors": ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]}]
    | | x  0dec01379d3b
    | |/     Obsfate: [{"markers": [["0dec01379d3be6318c470ead31b1fe7ae7cb53d5", ["b7ea6d14e664bdc8922221f7992631b50da3fb07"], 0, [["operation", "amend"], ["user", "test"]], [0.0, 0], null]], "successors": ["b7ea6d14e664bdc8922221f7992631b50da3fb07"]}]
    | x  471f378eab4c
    |/     Obsfate: [{"markers": [["471f378eab4c5e25f6c77f785b27c936efb22874", ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"], 0, [["user", "test"]], [0.0, 0], null]], "successors": ["eb5a0daa21923bbf8caeb2c42085b9e463861fd0"]}]
    o  ea207398892e"""

# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    @  eb5a0daa2192
    |
    | x  b7ea6d14e664
    | |    Obsfate: rewritten as 4:eb5a0daa2192
    | | x  0dec01379d3b
    | |/     Obsfate: rewritten using amend as 3:b7ea6d14e664
    | x  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    @  eb5a0daa2192
    |
    | x  b7ea6d14e664
    | |    Obsfate: rewritten as 4:eb5a0daa2192
    | | x  0dec01379d3b
    | |/     Obsfate: rewritten using amend as 3:b7ea6d14e664
    | x  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    @  eb5a0daa2192
    |
    | x  b7ea6d14e664
    | |    Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000)
    | | x  0dec01379d3b
    | |/     Obsfate: rewritten using amend as 3:b7ea6d14e664 by test (at 1970-01-01 00:00 +0000)
    | x  471f378eab4c
    |/     Obsfate: rewritten as 4:eb5a0daa2192 by test (at 1970-01-01 00:00 +0000)
    o  ea207398892e"""
sh % "hg log -G -T default --hidden" == r"""
    @  changeset:   4:eb5a0daa2192
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     C0
    |
    | x  changeset:   3:b7ea6d14e664
    | |  parent:      1:471f378eab4c
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  obsolete:    rewritten as 4:eb5a0daa2192
    | |  summary:     B1
    | |
    | | x  changeset:   2:0dec01379d3b
    | |/   user:        test
    | |    date:        Thu Jan 01 00:00:00 1970 +0000
    | |    obsolete:    rewritten using amend as 3:b7ea6d14e664
    | |    summary:     B0
    | |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten as 4:eb5a0daa2192
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Test template with pushed and pulled obs markers
# ================================================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/templates-local-remote-markers-1"'
sh % 'cd "$TESTTMP/templates-local-remote-markers-1"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % 'hg clone "$TESTTMP/templates-local-remote-markers-1" "$TESTTMP/templates-local-remote-markers-2"' == r"""
    updating to branch default
    2 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % 'cd "$TESTTMP/templates-local-remote-markers-2"'
sh % "hg log --hidden -G" == r"""
    @  changeset:   1:471f378eab4c
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % 'cd "$TESTTMP/templates-local-remote-markers-1"'
sh % "hg commit --amend -m A1"
sh % "hg commit --amend -m A2"
sh % "hg log --hidden -G" == r"""
    @  changeset:   3:7a230b46bf61
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | x  changeset:   2:fdf9bde5129a
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 3:7a230b46bf61
    |    summary:     A1
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:fdf9bde5129a
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % 'cd "$TESTTMP/templates-local-remote-markers-2"'
sh % "hg pull" == r"""
    pulling from $TESTTMP/templates-local-remote-markers-1
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 0 changes to 1 files
    2 new obsolescence markers
    obsoleted 1 changesets"""
sh % "hg log --hidden -G" == r"""
    o  changeset:   2:7a230b46bf61
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | @  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:7a230b46bf61
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

sh % "hg debugobsolete" == r"""
    471f378eab4c5e25f6c77f785b27c936efb22874 fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'}
    fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e 7a230b46bf61e50b30308c6cfd7bd1269ef54702 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'amend', 'user': 'test'}"""

# Check templates
# ---------------

# Predecessors template should show current revision as it is the working copy
sh % "hg tlog" == r"""
    o  7a230b46bf61
    |    Predecessors: 1:471f378eab4c
    |    semi-colon: 1:471f378eab4c
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | @  471f378eab4c
    |/     Successors: 2:7a230b46bf61
    |      multi-line: 2:7a230b46bf61
    |      json: [["7a230b46bf61e50b30308c6cfd7bd1269ef54702"]]
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    o  7a230b46bf61
    |
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:7a230b46bf61 by test (between 1970-01-01 00:00 +0000 and 1970-01-01 00:00 +0000);
    o  ea207398892e"""
sh % "hg up 'desc(A2)'" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Predecessors template should show no predecessors as they are non visible
sh % "hg tlog" == r"""
    @  7a230b46bf61
    |
    o  ea207398892e"""

sh % "hg fatelog" == r"""
    @  7a230b46bf61
    |
    o  ea207398892e"""
# Predecessors template should show all predecessors as we force their display
# with --hidden
sh % "hg tlog --hidden" == r"""
    @  7a230b46bf61
    |    Predecessors: 1:471f378eab4c
    |    semi-colon: 1:471f378eab4c
    |    json: ["471f378eab4c5e25f6c77f785b27c936efb22874"]
    |    map: 1:471f378eab4c5e25f6c77f785b27c936efb22874
    | x  471f378eab4c
    |/     Successors: 2:7a230b46bf61
    |      multi-line: 2:7a230b46bf61
    |      json: [["7a230b46bf61e50b30308c6cfd7bd1269ef54702"]]
    o  ea207398892e"""

sh % "hg fatelog --hidden" == r"""
    @  7a230b46bf61
    |
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:7a230b46bf61 by test (between 1970-01-01 00:00 +0000 and 1970-01-01 00:00 +0000);
    o  ea207398892e"""

# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    @  7a230b46bf61
    |
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:7a230b46bf61
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    @  7a230b46bf61
    |
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:7a230b46bf61
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    @  7a230b46bf61
    |
    | x  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:7a230b46bf61 by test (between 1970-01-01 00:00 +0000 and 1970-01-01 00:00 +0000)
    o  ea207398892e"""
sh % "hg log -G -T default --hidden" == r"""
    @  changeset:   2:7a230b46bf61
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A2
    |
    | x  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:7a230b46bf61
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Test templates with pruned commits
# ==================================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/templates-local-prune"'
sh % 'cd "$TESTTMP/templates-local-prune"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % "hg debugobsolete --record-parent ." == "obsoleted 1 changesets"

# Check output
# ------------

sh % "hg up 'desc(A0)' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg tlog" == r"""
    @  471f378eab4c
    |
    o  ea207398892e"""
sh % "hg fatelog" == r"""
    @  471f378eab4c
    |    Obsfate: pruned by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
# Test templates with multiple pruned commits
# ===========================================

# Test setup
# ----------

sh % 'hg init "$TESTTMP/multiple-local-prune"'
sh % 'cd "$TESTTMP/multiple-local-prune"'
sh % "mkcommit ROOT"
sh % "mkcommit A0"
sh % "hg commit --amend -m A1"
sh % "hg debugobsolete --record-parent ." == "obsoleted 1 changesets"

sh % "hg up -r 'desc(A0)' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg commit --amend -m A2"
sh % "hg debugobsolete --record-parent ." == "obsoleted 1 changesets"

# Check output
# ------------

sh % "hg up 'desc(A0)' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg tlog" == r"""
    @  471f378eab4c
    |
    o  ea207398892e"""
# todo: the obsfate output is not ideal
sh % "hg fatelog" == r"""
    @  471f378eab4c
    |    Obsfate: pruned;
    o  ea207398892e"""
sh % "hg fatelog --hidden" == r"""
    x  65b757b745b9
    |    Obsfate: pruned by test (at 1970-01-01 00:00 +0000);
    | x  fdf9bde5129a
    |/     Obsfate: pruned by test (at 1970-01-01 00:00 +0000);
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a by test (at 1970-01-01 00:00 +0000); rewritten using amend as 3:65b757b745b9 by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""
# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    x  65b757b745b9
    |    Obsfate: pruned
    | x  fdf9bde5129a
    |/     Obsfate: pruned
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a
    |      Obsfate: rewritten using amend as 3:65b757b745b9
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    x  65b757b745b9
    |    Obsfate: pruned
    | x  fdf9bde5129a
    |/     Obsfate: pruned
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a
    |      Obsfate: rewritten using amend as 3:65b757b745b9
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    x  65b757b745b9
    |    Obsfate: pruned by test (at 1970-01-01 00:00 +0000)
    | x  fdf9bde5129a
    |/     Obsfate: pruned by test (at 1970-01-01 00:00 +0000)
    | @  471f378eab4c
    |/     Obsfate: rewritten using amend as 2:fdf9bde5129a by test (at 1970-01-01 00:00 +0000)
    |      Obsfate: rewritten using amend as 3:65b757b745b9 by test (at 1970-01-01 00:00 +0000)
    o  ea207398892e"""

sh % "hg log -G -T default --hidden" == r"""
    x  changeset:   3:65b757b745b9
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  obsolete:    pruned
    |  summary:     A2
    |
    | x  changeset:   2:fdf9bde5129a
    |/   parent:      0:ea207398892e
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    pruned
    |    summary:     A1
    |
    | @  changeset:   1:471f378eab4c
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    rewritten using amend as 2:fdf9bde5129a
    |    obsolete:    rewritten using amend as 3:65b757b745b9
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""

# Test templates with splitted and pruned commit
# ==============================================

sh % 'hg init "$TESTTMP/templates-local-split-prune"'
sh % 'cd "$TESTTMP/templates-local-split-prune"'
sh % "mkcommit ROOT"
sh % "echo 42" >> "a"
sh % "echo 43" >> "b"
sh % "hg commit -A -m A0" == r"""
    adding a
    adding b"""
sh % "hg log --hidden -G" == r"""
    @  changeset:   1:471597cad322
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Simulate split
sh % "hg up -r 'desc(ROOT)'" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo 42" >> "a"
sh % "hg commit -A -m A1" == "adding a"
sh % "echo 43" >> "b"
sh % "hg commit -A -m A2" == "adding b"
sh % "hg debugobsolete 1 2 3" == "obsoleted 1 changesets"

# Simulate prune
sh % "hg debugobsolete --record-parent ." == "obsoleted 1 changesets"

sh % "hg log --hidden -G" == r"""
    @  changeset:   3:0d0ef4bdf70e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  obsolete:    pruned
    |  summary:     A2
    |
    o  changeset:   2:617adc3a144c
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A1
    |
    | x  changeset:   1:471597cad322
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    split as 2:617adc3a144c, 3:0d0ef4bdf70e
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
# Check templates
# ---------------

sh % "hg up 'desc(\"A0\")' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# todo: the obsfate output is not ideal
sh % "hg fatelog" == r"""
    o  617adc3a144c
    |
    | @  471597cad322
    |/     Obsfate: pruned;
    o  ea207398892e"""
sh % "hg up -r 'desc(\"A2\")' --hidden" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "hg fatelog --hidden" == r"""
    @  0d0ef4bdf70e
    |    Obsfate: pruned by test (at 1970-01-01 00:00 +0000);
    o  617adc3a144c
    |
    | x  471597cad322
    |/     Obsfate: split as 2:617adc3a144c, 3:0d0ef4bdf70e by test (at 1970-01-01 00:00 +0000);
    o  ea207398892e"""

# Check other fatelog implementations
# -----------------------------------

sh % "hg fatelogkw --hidden -q" == r"""
    @  0d0ef4bdf70e
    |    Obsfate: pruned
    o  617adc3a144c
    |
    | x  471597cad322
    |/     Obsfate: split as 2:617adc3a144c, 3:0d0ef4bdf70e
    o  ea207398892e"""
sh % "hg fatelogkw --hidden" == r"""
    @  0d0ef4bdf70e
    |    Obsfate: pruned
    o  617adc3a144c
    |
    | x  471597cad322
    |/     Obsfate: split as 2:617adc3a144c, 3:0d0ef4bdf70e
    o  ea207398892e"""
sh % "hg fatelogkw --hidden -v" == r"""
    @  0d0ef4bdf70e
    |    Obsfate: pruned by test (at 1970-01-01 00:00 +0000)
    o  617adc3a144c
    |
    | x  471597cad322
    |/     Obsfate: split as 2:617adc3a144c, 3:0d0ef4bdf70e by test (at 1970-01-01 00:00 +0000)
    o  ea207398892e"""
sh % "hg log -G -T default --hidden" == r"""
    @  changeset:   3:0d0ef4bdf70e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  obsolete:    pruned
    |  summary:     A2
    |
    o  changeset:   2:617adc3a144c
    |  parent:      0:ea207398892e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A1
    |
    | x  changeset:   1:471597cad322
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    obsolete:    split as 2:617adc3a144c, 3:0d0ef4bdf70e
    |    summary:     A0
    |
    o  changeset:   0:ea207398892e
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     ROOT"""
sh % "hg fatelogcount --hidden -q" == r"""
    @  0d0ef4bdf70e 1 succsandmarkers
    |
    o  617adc3a144c
    |
    | x  471597cad322 1 succsandmarkers
    |/
    o  ea207398892e"""
