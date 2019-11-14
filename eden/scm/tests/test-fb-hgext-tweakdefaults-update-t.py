# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
tweakdefaults=
rebase=
[experimental]
updatecheck=noconflict
""" >> "$HGRCPATH"
sh % "setconfig 'ui.suggesthgprev=True'"

# Set up the repository.
sh % "hg init repo"
sh % "cd repo"
sh % "hg debugbuilddag -m '+4 *3 +1'"
sh % "hg log --graph -r '0::' -T '{rev}'" == r"""
    o  5
    |
    o  4
    |
    | o  3
    | |
    | o  2
    |/
    o  1
    |
    o  0"""

sh % "hg up 3" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Make an uncommitted change.
sh % "echo foo" > "foo"
sh % "hg add foo"
sh % "hg st" == "A foo"

# Can always update to current commit.
sh % "hg up ." == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Abort with --check set, succeed with --merge
sh % "hg up 2 --check" == r"""
    abort: uncommitted changes
    [255]"""
sh % "hg up --merge 2" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Updates to other branches should fail without --merge.
sh % "hg up 4 --check" == r"""
    abort: uncommitted changes
    [255]"""
sh % "hg up --merge 4" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Certain flags shouldn't work together.
sh % "hg up --check --merge 3" == r"""
    abort: can only specify one of -C/--clean, -c/--check, or -m/--merge
    [255]"""
sh % "hg up --check --clean 3" == r"""
    abort: can only specify one of -C/--clean, -c/--check, or -m/--merge
    [255]"""
sh % "hg up --clean --merge 3" == r"""
    abort: can only specify one of -C/--clean, -c/--check, or -m/--merge
    [255]"""

# --clean should work as expected.
sh % "hg st" == "A foo"
sh % "hg up --clean 3" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg st" == "? foo"
sh % "enable amend"
sh % "hg update '.^'" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    hint[update-prev]: use 'hg prev' to move to the parent changeset
    hint[hint-ack]: use 'hg hint --ack update-prev' to silence these hints"""
