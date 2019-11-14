# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh.enable("remotenames")
sh.setconfig("infinitepush.branchpattern=re:draft.*", "visibility.enabled=1")

sh.newrepo("server")
sh.setconfig("treemanifest.server=1")

sh % "drawdag" << r"""
B C
|/
A
"""

sh % "hg log -Gr 'all()' -T '{desc} {node}'" == r"""
    o  C dc0947a82db884575bb76ea10ac97b08536bfa03
    |
    | o  B 112478962961147124edd43549aedd1a335e44bf
    |/
    o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0"""

sh % 'hg book -r "$A" book/a'
sh % 'hg book -r "$B" book/b'
sh % 'hg book -r "$C" draft/c'

sh % 'cd "$TESTTMP"'
sh % "hg clone -q --pull server client"
sh % "cd client"

sh % "hg pull -qr book/a"
sh % "hg pull -qr draft/c"

sh % "hg dbsh -y" << r"""
publicnodes, draftnodes = cl._remotenodes()
hex = m.node.hex
for node in sorted(publicnodes):
    ui.write("public %s\n" % hex(node))
for node in draftnodes:
    ui.write("draft  %s\n" % hex(node))
""" == r"""
    public 112478962961147124edd43549aedd1a335e44bf
    public 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
    draft  dc0947a82db884575bb76ea10ac97b08536bfa03"""

# Remove book/b. 'B' can disappear from heads, if narrow-heads is set.

sh % "hg --cwd ../server bookmark --delete book/b"
sh % "hg pull -q"

sh % "hg log -r 'head()' -T '{desc}' --config experimental.narrow-heads=0" == "BC"
sh % "hg log -r 'head()' -T '{desc}' --config experimental.narrow-heads=1" == r"""
    migrating repo to new-style visibility and phases
    (this does not affect most workflows; post in Source Control @ FB if you have issues)
    C"""
