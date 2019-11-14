# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "cat" << r"""
[extensions]
rebase=
histedit=

[alias]
tglog = log -G --template "{rev}: {node|short} '{desc}' {branches}\n"
""" >> "$HGRCPATH"


sh % "hg init a"
sh % "cd a"

sh % "echo C1" > "C1"
sh % "hg ci -Am C1" == "adding C1"

sh % "echo C2" > "C2"
sh % "hg ci -Am C2" == "adding C2"

sh % "cd .."

sh % "hg clone a b" == r"""
    updating to branch default
    2 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

sh % "hg clone a c" == r"""
    updating to branch default
    2 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

sh % "cd b"

sh % "echo L1" > "L1"
sh % "hg ci -Am L1" == "adding L1"


sh % "cd ../a"

sh % "echo R1" > "R1"
sh % "hg ci -Am R1" == "adding R1"


sh % "cd ../b"

# Now b has one revision to be pulled from a:

sh % "hg pull --rebase" == r'''
    pulling from $TESTTMP/a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 77ae9631bcca
    rebasing ff8d69a621f9 "L1"'''

sh % "tglog" == r"""
    @  4: d80cc2da061e 'L1'
    |
    o  3: 77ae9631bcca 'R1'
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
# Re-run:

sh % "hg pull --rebase" == r"""
    pulling from $TESTTMP/a
    searching for changes
    no changes found"""

# Abort pull early if working dir is not clean:

sh % "echo L1-mod" > "L1"
sh % "hg pull --rebase" == r"""
    abort: uncommitted changes
    (cannot pull with rebase: please commit or shelve your changes first)
    [255]"""
sh % "hg update --clean --quiet"

# Abort pull early if another operation (histedit) is in progress:

sh % "hg histedit . -q --commands -" << r"""
edit d80cc2da061e histedit: generate unfinished state
""" == r"""
    Editing (d80cc2da061e), you may commit or record as needed now.
    (hg histedit --continue to resume)
    [1]"""
sh % "hg pull --rebase" == r"""
    abort: histedit in progress
    (use 'hg histedit --continue' or 'hg histedit --abort')
    [255]"""
sh % "hg histedit --abort --quiet"

# Abort pull early with pending uncommitted merge:

sh % "cd .."
sh % "hg clone --noupdate c d"
sh % "cd d"
sh % "tglog" == r"""
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
sh % "hg update --quiet 0"
sh % "echo M1" > "M1"
sh % "hg commit --quiet -Am M1"
sh % "hg update --quiet 1"
sh % "hg merge 2" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg pull --rebase" == r"""
    abort: outstanding uncommitted merge
    (cannot pull with rebase: please commit or shelve your changes first)
    [255]"""
sh % "hg update --clean --quiet"

# Invoke pull --rebase and nothing to rebase:

sh % "cd ../c"

sh % "hg book norebase"
sh % "hg pull --rebase" == r"""
    pulling from $TESTTMP/a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 77ae9631bcca
    nothing to rebase - updating instead
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    updating bookmark norebase"""

sh % "tglog -l 1" == r"""
    @  2: 77ae9631bcca 'R1' norebase
    |
    ~"""

# pull --rebase --update should ignore --update:

sh % "hg pull --rebase --update" == r"""
    pulling from $TESTTMP/a
    searching for changes
    no changes found"""

# pull --rebase doesn't update if nothing has been pulled:

sh % "hg up -q 1"

sh % "hg pull --rebase" == r"""
    pulling from $TESTTMP/a
    searching for changes
    no changes found"""

sh % "tglog -l 1" == r"""
    o  2: 77ae9631bcca 'R1' norebase
    |
    ~"""

sh % "cd .."

# pull --rebase works when a specific revision is pulled (issue3619)

sh % "cd a"
sh % "tglog" == r"""
    @  2: 77ae9631bcca 'R1'
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
sh % "echo R2" > "R2"
sh % "hg ci -Am R2" == "adding R2"
sh % "echo R3" > "R3"
sh % "hg ci -Am R3" == "adding R3"
sh % "cd ../c"
sh % "tglog" == r"""
    o  2: 77ae9631bcca 'R1' norebase
    |
    @  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
sh % "echo L1" > "L1"
sh % "hg ci -Am L1" == "adding L1"
sh % "hg pull --rev tip --rebase" == r'''
    pulling from $TESTTMP/a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 2 changesets with 2 changes to 2 files
    new changesets 31cd3a05214e:770a61882ace
    rebasing ff8d69a621f9 "L1"'''
sh % "tglog" == r"""
    @  6: 518d153c0ba3 'L1'
    |
    o  5: 770a61882ace 'R3'
    |
    o  4: 31cd3a05214e 'R2'
    |
    o  2: 77ae9631bcca 'R1' norebase
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
# pull --rebase works with bundle2 turned on

sh % "cd ../a"
sh % "echo R4" > "R4"
sh % "hg ci -Am R4" == "adding R4"
sh % "tglog" == r"""
    @  5: 00e3b7781125 'R4'
    |
    o  4: 770a61882ace 'R3'
    |
    o  3: 31cd3a05214e 'R2'
    |
    o  2: 77ae9631bcca 'R1'
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
sh % "cd ../c"
sh % "hg pull --rebase" == r'''
    pulling from $TESTTMP/a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 00e3b7781125
    rebasing 518d153c0ba3 "L1"'''
sh % "tglog" == r"""
    @  8: 0d0727eb7ce0 'L1'
    |
    o  7: 00e3b7781125 'R4'
    |
    o  5: 770a61882ace 'R3'
    |
    o  4: 31cd3a05214e 'R2'
    |
    o  2: 77ae9631bcca 'R1' norebase
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""

# pull --rebase only update if there is nothing to rebase

sh % "cd ../a"
sh % "echo R5" > "R5"
sh % "hg ci -Am R5" == "adding R5"
sh % "tglog" == r"""
    @  6: 88dd24261747 'R5'
    |
    o  5: 00e3b7781125 'R4'
    |
    o  4: 770a61882ace 'R3'
    |
    o  3: 31cd3a05214e 'R2'
    |
    o  2: 77ae9631bcca 'R1'
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
sh % "cd ../c"
sh % "echo L2" > "L2"
sh % "hg ci -Am L2" == "adding L2"
sh % "hg up 'desc(L1)'" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg pull --rebase" == r'''
    pulling from $TESTTMP/a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 88dd24261747
    rebasing 0d0727eb7ce0 "L1"
    rebasing c1f58876e3bf "L2"'''
sh % "tglog" == r"""
    o  12: 6dc0ea5dcf55 'L2'
    |
    @  11: 864e0a2d2614 'L1'
    |
    o  10: 88dd24261747 'R5'
    |
    o  7: 00e3b7781125 'R4'
    |
    o  5: 770a61882ace 'R3'
    |
    o  4: 31cd3a05214e 'R2'
    |
    o  2: 77ae9631bcca 'R1' norebase
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""

# pull --rebase update (no rebase) use proper update:

# - warn about other head.

sh % "cd ../a"
sh % "echo R6" > "R6"
sh % "hg ci -Am R6" == "adding R6"
sh % "cd ../c"
sh % "hg up 'desc(R5)'" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg pull --rebase" == r'''
    pulling from $TESTTMP/a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    new changesets 65bc164c1d9b
    nothing to rebase - updating instead
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    updated to "65bc164c1d9b: R6"
    1 other heads for branch "default"'''
sh % "tglog" == r"""
    @  13: 65bc164c1d9b 'R6'
    |
    | o  12: 6dc0ea5dcf55 'L2'
    | |
    | o  11: 864e0a2d2614 'L1'
    |/
    o  10: 88dd24261747 'R5'
    |
    o  7: 00e3b7781125 'R4'
    |
    o  5: 770a61882ace 'R3'
    |
    o  4: 31cd3a05214e 'R2'
    |
    o  2: 77ae9631bcca 'R1' norebase
    |
    o  1: 783333faa078 'C2'
    |
    o  0: 05d58a0c15dd 'C1'"""
