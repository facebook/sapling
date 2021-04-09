# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Set up global extensions
sh % "cat" << r"""
[extensions]
rebase=
""" >> "$HGRCPATH"

# Create a repo without remotenames
sh % "hg init off"
sh % "cd off"
sh % "echo a" > "a"
sh % "hg ci -qAm a"
sh % "cd .."

# Clone repo and turn remotenames on
sh % "hg clone off on" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cat" << r"""
[extensions]
remotenames=
""" >> "on/.hg/hgrc"

# Ensure no crashes when working from repo with remotenames on
sh % "hg -R off bookmark foo"
sh % "cd on"

sh % "hg pull" == r"""
    pulling from $TESTTMP/off
    searching for changes
    no changes found"""

sh % "hg push --to bar --create" == r"""
    pushing rev cb9a9f314b8b to destination $TESTTMP/off bookmark bar
    searching for changes
    no changes found
    exporting bookmark bar
    [1]"""

sh % "hg pull --rebase" == r"""
    pulling from $TESTTMP/off
    searching for changes
    no changes found"""

sh % "cd .."

# Check for crashes when working from repo with remotenames off
sh % "cd off"

sh % "hg pull ../on" == r"""
    pulling from ../on
    searching for changes
    no changes found"""

sh % "cat" << r"""
[paths]
default = $TESTTMP/on
""" >> ".hg/hgrc"

sh % "hg pull" == r"""
    pulling from $TESTTMP/on
    searching for changes
    no changes found"""

sh % "hg push" == r"""
    pushing to $TESTTMP/on
    searching for changes
    no changes found
    [1]"""

sh % "hg pull --rebase" == r"""
    pulling from $TESTTMP/on
    searching for changes
    no changes found"""
