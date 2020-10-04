# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init rep"
sh % "cd rep"
sh % "mkdir dir"
sh % "touch foo dir/bar"
sh % "hg -v addremove" == r"""
    adding dir/bar
    adding foo"""
sh % "hg -v commit -m 'add 1'" == r"""
    committing files:
    dir/bar
    foo
    committing manifest
    committing changelog
    committed changeset 0:6f7f953567a2"""
sh % "cd dir/"
sh % "touch ../foo_2 bar_2"
sh % "hg -v addremove" == r"""
    adding dir/bar_2
    adding foo_2"""
sh % "hg -v commit -m 'add 2'" == r"""
    committing files:
    dir/bar_2
    foo_2
    committing manifest
    committing changelog
    committed changeset 1:e65414bf35c5"""
sh % "cd .."
sh % "hg forget foo"
sh % "hg -v addremove" == "adding foo"
sh % "hg forget foo"

sh % "hg -v addremove nonexistent" == r"""
    nonexistent: $ENOENT$
    [1]"""

sh % "cd .."

sh % "hg init subdir"
sh % "cd subdir"
sh % "mkdir dir"
sh % "cd dir"
sh % "touch a.py"
sh % "hg addremove 'glob:*.py'" == "adding a.py"
sh % "hg forget a.py"
sh % "hg addremove -I 'glob:*.py'" == "adding a.py"
sh % "hg forget a.py"
sh % "hg addremove" == "adding dir/a.py"
sh % "cd .."
sh % "cd .."

sh % "hg init sim"
sh % "cd sim"
sh % "echo a" > "a"
sh % "echo a" >> "a"
sh % "echo a" >> "a"
sh % "echo c" > "c"
sh % "hg commit -Ama" == r"""
    adding a
    adding c"""
sh % "mv a b"
sh % "rm c"
sh % "echo d" > "d"
sh % "hg addremove -n -s 50" == r"""
    removing a
    adding b
    removing c
    adding d
    recording removal of a as rename to b (100% similar)"""
sh % "hg addremove -s 50" == r"""
    removing a
    adding b
    removing c
    adding d
    recording removal of a as rename to b (100% similar)"""
sh % "hg commit -mb"
sh % "cp b c"
sh % "hg forget b"
sh % "hg addremove -s 50" == r"""
    adding b
    adding c"""

sh % "rm c"

sh % "hg ci -A -m c nonexistent" == r"""
    nonexistent: $ENOENT$
    abort: failed to mark all new/missing files as added/removed
    [255]"""

sh % "hg st" == "! c"

sh % "hg forget c"
sh % "touch foo"
sh % "hg addremove" == "adding foo"
sh % "rm foo"
sh % "hg addremove" == "removing foo"

sh % "cd .."
