# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
amend=
cleanobsstore=
[experimental]
evolution=createmarkers
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"
sh % "hg debugbuilddag +5"
sh % "hg up -q tip"
sh % "hg prune -r ." == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    working directory now at 2dc09a01254d
    1 changesets pruned
    hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
    hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints"""
sh % "hg debugobsolete" == "bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}"
sh % "'HGUSER=baduser' hg prune -r ." == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    working directory now at 01241442b3c2
    1 changesets pruned
    hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
    hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints"""
sh % "hg debugobsolete" == r"""
    bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
    2dc09a01254db841290af0538aa52f6f52c776e3 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}"""

# Run any command (for example, status). Obsstore shouldn't be cleaned because it doesn't exceed the limit
sh % "hg --config 'cleanobsstore.badusernames=baduser' st"
sh % "hg debugobsolete" == r"""
    bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
    2dc09a01254db841290af0538aa52f6f52c776e3 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}"""

# Run any command again. This time it should be cleaned because we decreased the limit
sh % "hg --config 'cleanobsstore.badusernames=baduser' --config 'cleanobsstore.obsstoresizelimit=1' st"
sh % "hg debugobsolete" == "bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}"

# Create bad obsmarker again. Make sure it wasn't cleaned again
sh % "echo 1" >> "1"
sh % "hg add 1"
sh % "hg ci -q -m 1"
sh % "'HGUSER=baduser' hg prune -q -r ." == r"""
    hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
    hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints"""
sh % "hg debugobsolete" == r"""
    bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
    73bce0eaaf9d039023d1b34421aceab146636d3e 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}"""
sh % "hg --config 'cleanobsstore.badusernames=baduser' --config 'cleanobsstore.obsstoresizelimit=1' st"
sh % "hg debugobsolete" == r"""
    bebd167eb94d257ace0e814aeb98e6972ed2970d 0 {2dc09a01254db841290af0538aa52f6f52c776e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
    73bce0eaaf9d039023d1b34421aceab146636d3e 0 {01241442b3c2bf3211e593b549c655ea65b295e3} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'baduser'}"""
