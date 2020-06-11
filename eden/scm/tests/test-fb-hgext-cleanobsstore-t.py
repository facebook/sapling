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
sh % "hg debugobsolete" == ""
sh % "'HGUSER=baduser' hg prune -r ." == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    working directory now at 01241442b3c2
    1 changesets pruned
    hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
    hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints"""
sh % "hg debugobsolete" == ""

# Run any command (for example, status). Obsstore shouldn't be cleaned because it doesn't exceed the limit
sh % "hg --config 'cleanobsstore.badusernames=baduser' st"
sh % "hg debugobsolete" == ""

# Run any command again. This time it should be cleaned because we decreased the limit
sh % "hg --config 'cleanobsstore.badusernames=baduser' --config 'cleanobsstore.obsstoresizelimit=1' st"
sh % "hg debugobsolete" == ""

# Create bad obsmarker again. Make sure it wasn't cleaned again
sh % "echo 1" >> "1"
sh % "hg add 1"
sh % "hg ci -q -m 1"
sh % "'HGUSER=baduser' hg prune -q -r ." == r"""
    hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
    hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints"""
sh % "hg debugobsolete" == ""
sh % "hg --config 'cleanobsstore.badusernames=baduser' --config 'cleanobsstore.obsstoresizelimit=1' st"
sh % "hg debugobsolete" == ""
