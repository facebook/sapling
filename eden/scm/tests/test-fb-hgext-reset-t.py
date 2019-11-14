# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
reset=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "hg book foo"

# Soft reset should leave pending changes

sh % "echo y" >> "x"
sh % "hg commit -qAm y"
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == r"""
    @  66ee28d0328c foo
    |
    o  b292c1e3311f"""
sh % "hg reset '.^'" == "1 changesets pruned"
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == "@  b292c1e3311f foo"
sh % "hg diff" == r"""
    diff -r b292c1e3311f x
    --- a/x	Thu Jan 01 00:00:00 1970 +0000
    +++ b/x	* (glob)
    @@ -1,1 +1,2 @@
     x
    +y"""

# Clean reset should overwrite all changes

sh % "hg commit -qAm y"
sh % "hg reset --clean '.^'" == "1 changesets pruned"
sh % "hg diff"

# Reset should recover from backup bundles (with correct phase)

sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == "@  b292c1e3311f foo"
sh % "hg phase -p b292c1e3311f"
sh % "hg reset --clean 66ee28d0328c"
sh % "hg log -G -T '{node|short} {bookmarks} {phase}\\n'" == r"""
    @  66ee28d0328c foo draft
    |
    o  b292c1e3311f  public"""
sh % "hg phase -f -d b292c1e3311f"

# Reset should not strip reachable commits

sh % "hg book bar"
sh % "hg reset --clean '.^'"
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == r"""
    o  66ee28d0328c foo
    |
    @  b292c1e3311f bar"""

sh % "hg book -d bar"
sh % "hg up foo" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (activating bookmark foo)"""

# Reset to '.' by default

sh % "echo z" >> "x"
sh % "echo z" >> "y"
sh % "hg add y"
sh % "hg st" == r"""
    M x
    A y"""
sh % "hg reset"
sh % "hg st" == r"""
    M x
    ? y"""
sh % "hg reset -C"
sh % "hg st" == "? y"
sh % "rm y"

# Keep old commits

sh % "hg reset --keep '.^'"
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == r"""
    o  66ee28d0328c
    |
    @  b292c1e3311f foo"""
# Reset without a bookmark

sh % "hg up tip" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark foo)"""
sh % "hg book -d foo"
sh % "hg reset '.^'" == "1 changesets pruned"
sh % "hg book foo"

# Reset to bookmark with - in the name

sh % "hg reset 66ee28d0328c"
sh % "hg book foo-bar -r '.^'"
sh % "hg reset foo-bar" == "1 changesets pruned"
sh % "hg book -d foo-bar"

# Verify file status after reset

sh % "hg reset -C 66ee28d0328c"
sh % "touch toberemoved"
sh % "hg commit -qAm 'add file for removal'"
sh % "echo z" >> "x"
sh % "touch tobeadded"
sh % "hg add tobeadded"
sh % "hg rm toberemoved"
sh % "hg commit -m 'to be reset'"
sh % "hg reset '.^'" == "1 changesets pruned"
sh % "hg status" == r"""
    M x
    ! toberemoved
    ? tobeadded"""
sh % "hg reset -C 66ee28d0328c" == "1 changesets pruned"

# Reset + Obsolete tests

sh % "cat" << r"""
[extensions]
amend=
rebase=
[experimental]
evolution=all
""" >> ".hg/hgrc"
sh % "touch a"
sh % "hg commit -Aqm a"
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == r"""
    @  7f3a02b3e388 foo
    |
    o  66ee28d0328c
    |
    o  b292c1e3311f"""

# Reset prunes commits

sh % "hg reset -C '66ee28d0328c^'" == "2 changesets pruned"
sh % "hg log -r 66ee28d0328c" == r"""
    abort: hidden revision '66ee28d0328c'!
    (use --hidden to access hidden revisions)
    [255]"""
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == "@  b292c1e3311f foo"
sh % "hg reset -C 7f3a02b3e388"
sh % "hg log -G -T '{node|short} {bookmarks}\\n'" == r"""
    @  7f3a02b3e388 foo
    |
    o  66ee28d0328c
    |
    o  b292c1e3311f"""
# Reset to the commit your on is a no-op
sh % "hg status"
sh % "hg log -r . -T '{rev}\\n'" == "4"
sh % "hg reset ."
sh % "hg log -r . -T '{rev}\\n'" == "4"
sh % "hg debugdirstate" == r"""
    n 644          0 * a (glob)
    n 644          0 * tobeadded (glob)
    n 644          4 * x (glob)"""
