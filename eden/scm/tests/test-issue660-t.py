# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# https://bz.mercurial-scm.org/660 and:
# https://bz.mercurial-scm.org/322

sh % "hg init"
sh % "echo a" > "a"
sh % "mkdir b"
sh % "echo b" > "b/b"
sh % "hg commit -A -m 'a is file, b is dir'" == r"""
    adding a
    adding b/b"""

# File replaced with directory:

sh % "rm a"
sh % "mkdir a"
sh % "echo a" > "a/a"

# Should fail - would corrupt dirstate:

sh % "hg add a/a" == r"""
    abort: file 'a' in dirstate clashes with 'a/a'
    [255]"""

# Removing shadow:

sh % "hg rm --after a"

# Should succeed - shadow removed:

sh % "hg add a/a"

# Directory replaced with file:

sh % "rm -r b"
sh % "echo b" > "b"

# Should fail - would corrupt dirstate:

sh % "hg add b" == r"""
    abort: directory 'b' already in dirstate
    [255]"""

# Removing shadow:

sh % "hg rm --after b/b"

# Should succeed - shadow removed:

sh % "hg add b"

# Look what we got:

sh % "hg st" == r"""
    A a/a
    A b
    R a
    R b/b"""

# Revert reintroducing shadow - should fail:

sh % "rm -r a b"
sh % "hg revert b/b" == r"""
    abort: file 'b' in dirstate clashes with 'b/b'
    [255]"""

# Revert all - should succeed:

sh % "hg revert --all" == r"""
    undeleting a
    forgetting a/a
    forgetting b
    undeleting b/b"""

sh % "hg st"

# Issue3423:

sh % "hg forget a"
sh % "echo zed" > "a"
sh % "hg revert a"
sh % "hg st" == "? a.orig"
sh % "rm a.orig"

# addremove:

sh % "rm -r a b"
sh % "mkdir a"
sh % "echo a" > "a/a"
sh % "echo b" > "b"

sh % "hg addremove -s 0" == r"""
    removing a
    adding a/a
    adding b
    removing b/b"""

sh % "hg st" == r"""
    A a/a
    A b
    R a
    R b/b"""

# commit:

sh % "hg ci -A -m 'a is dir, b is file'"
sh % "hg st --all" == r"""
    C a/a
    C b"""

# Long directory replaced with file:

sh % "mkdir d"
sh % "mkdir d/d"
sh % "echo d" > "d/d/d"
sh % "hg commit -A -m 'd is long directory'" == "adding d/d/d"

sh % "rm -r d"
sh % "echo d" > "d"

# Should fail - would corrupt dirstate:

sh % "hg add d" == r"""
    abort: directory 'd' already in dirstate
    [255]"""

# Removing shadow:

sh % "hg rm --after d/d/d"

# Should succeed - shadow removed:

sh % "hg add d"
sh % "hg ci -md"

# Update should work at least with clean working directory:

sh % "rm -r a b d"
sh % "hg up -r 0" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "hg st --all" == r"""
    C a
    C b/b"""

sh % "rm -r a b"
sh % "hg up -r 1" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "hg st --all" == r"""
    C a/a
    C b"""
