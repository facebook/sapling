# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# With copied file using the heuristics copytracing:

sh % "enable mergedriver"

sh % "newrepo"
sh % "enable copytrace amend"
sh % "setconfig 'copytrace.draftusefullcopytrace=0' 'experimental.copytrace=off' 'copytrace.fastcopytrace=1' 'experimental.mergedriver=python:$TESTTMP/m.py'"

sh % "drawdag" << r"""
B C
|/
A
|
Z
"""

sh % "cat" << r"""
def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
    ui.write("unresolved: %r\n" % (sorted(mergestate.unresolved())))
def conclude(ui, repo, hooktype, mergestate, wctx, labels):
    pass
""" > "$TESTTMP/m.py"

sh % "hg up -q $B"
#  (trigger amend copytrace code path)
sh % "hg cp A D"
sh % "hg cp A E"
sh % "hg amend -m B2 -d '0 0'"
sh % "hg bookmark -i book-B"

# Do the merge:

sh % "hg up -q $C"
sh % "hg graft book-B" == 'grafting 4:b55db8435dc2 "B2" (book-B)'

sh % "hg status"
sh % "hg log -r . -T '{desc}\\n' --stat" == r"""
    B2
     B |  1 +
     D |  1 +
     E |  1 +
     3 files changed, 3 insertions(+), 0 deletions(-)"""

# Run again with heuristics copytrace disabled:

sh % "setconfig 'extensions.copytrace=!' 'experimental.copytrace=on' 'copytrace.fastcopytrace=0'"

sh % "hg up -q $C"
sh % "hg graft book-B" == 'grafting 4:b55db8435dc2 "B2" (book-B)'
