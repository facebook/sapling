# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "enable rebase"
sh % "newrepo"
sh % "drawdag" << r"""
D    # A/A=1\n
|    # B/A=(removed)
B C  # B/Renamed=1\n
|/   # C/A=2\n
A
"""

sh % "hg up -q $C"
sh % "hg rebase -r $C -d $D '--config=ui.interactive=1' '--config=experimental.copytrace=off'" << r"""
r
Renamed
""" == r"""
    rebasing 85b47c0eb942 "C"
    other [source] changed A which local [dest] deleted
    use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? r
    path 'A' in commit 85b47c0eb942 was renamed to [what path] in commit ed4ad4ec6472 ? Renamed
    merging Renamed"""

sh % "hg log -Gp -T '{desc}\\n' --git Renamed A" == r"""
    @  C
    :  diff --git a/Renamed b/Renamed
    :  --- a/Renamed
    :  +++ b/Renamed
    :  @@ -1,1 +1,1 @@
    :  -1
    :  +2
    :
    o  B
    |  diff --git a/A b/A
    |  deleted file mode 100644
    |  --- a/A
    |  +++ /dev/null
    |  @@ -1,1 +0,0 @@
    |  -1
    |  diff --git a/Renamed b/Renamed
    |  new file mode 100644
    |  --- /dev/null
    |  +++ b/Renamed
    |  @@ -0,0 +1,1 @@
    |  +1
    |
    o  A
       diff --git a/A b/A
       new file mode 100644
       --- /dev/null
       +++ b/A
       @@ -0,0 +1,1 @@
       +1"""

# status should not show "! A"
sh % "hg status" == ""
