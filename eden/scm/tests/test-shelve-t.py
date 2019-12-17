# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from edenscm.mercurial import extensions, hg, obsolete
from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


# TODO: Make this test compatibile with obsstore enabled.
sh % "setconfig 'experimental.evolution='"
sh % ". helpers-usechg.sh"


sh % "cat" << r"""
[extensions]
strip =
shelve=
[defaults]
diff = --nodates --git
qnew = --date '0 0'
[shelve]
maxbackups = 2
[experimental]
evolution=createmarkers
""" >> "$HGRCPATH"

# Make sure obs-based shelve can be used with an empty repo
sh % 'cd "$TESTTMP"'
sh % "hg init obsrepo"
sh % "cd obsrepo"

sh % "mkdir a b"
sh % "echo a" > "a/a"
sh % "echo b" > "b/b"
sh % "echo c" > "c"
sh % "echo d" > "d"
sh % "echo x" > "x"
sh % "hg addremove -q"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 5 files removed, 0 files unresolved"""
sh % "hg shelve --list" == "default * (changes in empty repository) (glob)"
sh % "hg revert --all"
sh % "hg unshelve" == "unshelving change 'default'"
sh % "hg diff" == r"""
    diff --git a/a/a b/a/a
    new file mode 100644
    --- /dev/null
    +++ b/a/a
    @@ -0,0 +1,1 @@
    +a
    diff --git a/b/b b/b/b
    new file mode 100644
    --- /dev/null
    +++ b/b/b
    @@ -0,0 +1,1 @@
    +b
    diff --git a/c b/c
    new file mode 100644
    --- /dev/null
    +++ b/c
    @@ -0,0 +1,1 @@
    +c
    diff --git a/d b/d
    new file mode 100644
    --- /dev/null
    +++ b/d
    @@ -0,0 +1,1 @@
    +d
    diff --git a/x b/x
    new file mode 100644
    --- /dev/null
    +++ b/x
    @@ -0,0 +1,1 @@
    +x"""
sh % "hg ci -qm 'initial commit'"
sh % "hg shelve" == r"""
    nothing changed
    [1]"""

# Make sure shelve files were backed up
sh % "ls .hg/shelve-backup" == r"""
    default.oshelve
    default.patch"""

sh % "echo n" > "n"
sh % "hg add n"
sh % "hg commit n -m second"

# Shelve a change that we will delete later
sh % "echo a" >> "a/a"
sh % "hg shelve" == r"""
    shelved as default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# Set up some more complex shelve changes to shelve
sh % "echo a" >> "a/a"
sh % "hg mv b b.rename" == "moving b/b to b.rename/b (glob)"
sh % "hg cp c c.copy"
sh % "hg status -C" == r"""
    M a/a
    A b.rename/b
      b/b
    A c.copy
      c
    R b/b"""

# The common case - no options or filenames
sh % "hg shelve" == r"""
    shelved as default-01
    2 files updated, 0 files merged, 2 files removed, 0 files unresolved"""
sh % "hg status -C"

# Ensure that our shelved changes exist
sh % "hg shelve -l" == r"""
    default-01 * shelve changes to: second (glob)
    default * shelve changes to: second (glob)"""
sh % "hg shelve -l -p default" == r"""
    default * shelve changes to: second (glob)

    diff --git a/a/a b/a/a
    --- a/a/a
    +++ b/a/a
    @@ -1,1 +1,2 @@
     a
    +a"""

sh % "hg shelve --list --addremove" == r"""
    abort: options '--list' and '--addremove' may not be used together
    [255]"""

# Delete our older shelved change
sh % "hg shelve -d default"

# Ensure shelve backups aren't overwritten
sh % "ls .hg/shelve-backup/" == r"""
    default-1.oshelve
    default-1.patch
    default.oshelve
    default.patch"""

# Local edits should not prevent a shelved change from applying
sh % "printf 'z\\na\\n'" > "a/a"
sh % "hg unshelve --keep" == r"""
    unshelving change 'default-01'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing 4893561a85b4 "shelve changes to: second"
    merging a/a"""

sh % "hg revert --all -q"
sh % "rm a/a.orig b.rename/b c.copy"

# Apply it and make sure our state is as expected
# (this also tests that same timestamp prevents backups from being
# removed, even though there are more than 'maxbackups' backups)
sh % "test -f .hg/shelve-backup/default.patch"
sh % "test -f .hg/shelve-backup/default-1.patch"
for name in ["default.patch", "default-1.patch"]:
    path = ".hg/shelve-backup/%s" % name
    os.utime(path, (0, 0))

sh % "hg unshelve" == "unshelving change 'default-01'"
sh % "hg status -C" == r"""
    M a/a
    A b.rename/b
      b/b
    A c.copy
      c
    R b/b"""
sh % "hg shelve -l"

# (both of default.oshelve and default-1.oshelve should be still kept,
# because it is difficult to decide actual order of them from same timestamp)
sh % "ls .hg/shelve-backup/" == r"""
    default-01.oshelve
    default-01.patch
    default-1.oshelve
    default-1.patch
    default.oshelve
    default.patch"""
sh % "hg unshelve" == r"""
    abort: no shelved changes to apply!
    [255]"""
sh % "hg unshelve foo" == r"""
    abort: shelved change 'foo' not found
    [255]"""

# Named shelves, specific filenames, and "commit messages" should all work
# (this tests also that editor is invoked, if '--edit' is specified)
sh % "hg status -C" == r"""
    M a/a
    A b.rename/b
      b/b
    A c.copy
      c
    R b/b"""
sh % "'HGEDITOR=cat' hg shelve -q -n wibble -m wat -e a" == r"""
    wat


    HG: Enter commit message.  Lines beginning with 'HG:' are removed.
    HG: Leave message empty to abort commit.
    HG: --
    HG: user: test
    HG: branch 'default'
    HG: changed a/a"""

# Expect "a" to no longer be present, but status otherwise unchanged
sh % "hg status -C" == r"""
    A b.rename/b
      b/b
    A c.copy
      c
    R b/b"""
sh % "hg shelve -l --stat" == r"""
    wibble * wat (glob)
     a/a |  1 +
     1 files changed, 1 insertions(+), 0 deletions(-)"""

# And now "a/a" should reappear
sh % "cd a"
sh % "hg unshelve -q wibble"
sh % "cd .."
sh % "hg status -C" == r"""
    M a/a
    A b.rename/b
      b/b
    A c.copy
      c
    R b/b"""

# Ensure old shelve backups are being deleted automatically
sh % "ls .hg/shelve-backup/" == r"""
    default-01.oshelve
    default-01.patch
    wibble.oshelve
    wibble.patch"""

# Cause unshelving to result in a merge with 'a' conflicting
sh % "hg shelve -q"
sh % "echo 'c'" >> "a/a"
sh % "hg commit -m second"
sh % "hg tip --template '{files}\\n'" == "a/a"

# Add an unrelated change that should be preserved
sh % "mkdir foo"
sh % "echo foo" > "foo/foo"
sh % "hg add foo/foo"

# Force a conflicted merge to occur
sh % "hg unshelve" == r"""
    unshelving change 'default'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing 4893561a85b4 "shelve changes to: second"
    merging a/a
    warning: 1 conflicts while merging a/a! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""

# Ensure that we have a merge with unresolved conflicts
sh % "hg heads -q --template '{rev}\\n'" == r"""
    11
    4"""
sh % "hg parents -q --template '{rev}\\n'" == r"""
    11
    4"""
sh % "hg status" == r"""
    M a/a
    M b.rename/b
    M c.copy
    R b/b
    ? a/a.orig"""
sh % "hg diff" == r"""
    diff --git a/a/a b/a/a
    --- a/a/a
    +++ b/a/a
    @@ -1,2 +1,6 @@
     a
    +<<<<<<< dest:   83ed350dc2d6 - test: pending changes temporary commit
     c
    +=======
    +a
    +>>>>>>> source: 4893561a85b4 - test: shelve changes to: second
    diff --git a/b/b b/b.rename/b
    rename from b/b
    rename to b.rename/b
    diff --git a/c b/c.copy
    copy from c
    copy to c.copy"""
sh % "hg resolve -l" == "U a/a"

sh % "hg shelve" == r"""
    abort: unshelve already in progress
    (use 'hg unshelve --continue' or 'hg unshelve --abort')
    [255]"""

# Abort the unshelve and be happy
sh % "hg status" == r"""
    M a/a
    M b.rename/b
    M c.copy
    R b/b
    ? a/a.orig"""
sh % "hg unshelve -a" == r"""
    rebase aborted
    unshelve of 'default' aborted"""
sh % "hg heads -q" == "10:ceefc37abe1e"
sh % "hg parents -T '{node|short}'" == "ceefc37abe1e"
sh % "hg resolve -l"
sh % "hg status" == r"""
    A foo/foo
    ? a/a.orig"""

# Try to continue with no unshelve underway
sh % "hg unshelve -c" == r"""
    abort: no unshelve in progress
    [255]"""
sh % "hg status" == r"""
    A foo/foo
    ? a/a.orig"""

# Redo the unshelve to get a conflict
sh % "hg unshelve -q" == r"""
    warning: 1 conflicts while merging a/a! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""

# Attempt to continue
sh % "hg unshelve -c" == r"""
    abort: unresolved conflicts, can't continue
    (see 'hg resolve', then 'hg unshelve --continue')
    [255]"""
sh % "hg revert -r . a/a"
sh % "hg resolve -m a/a" == r"""
    (no more unresolved files)
    continue: hg unshelve --continue"""
sh % "hg commit -m 'commit while unshelve in progress'" == r"""
    abort: unshelve already in progress
    (use 'hg unshelve --continue' or 'hg unshelve --abort')
    [255]"""
sh % "hg graft --continue" == r"""
    abort: no graft in progress
    (continue: hg unshelve --continue)
    [255]"""
sh % "hg unshelve -c --trace" == r"""
    rebasing 4893561a85b4 "shelve changes to: second"
    unshelve of 'default' complete"""

# Ensure the repo is as we hope
sh % "hg parents -T '{node|short}'" == "ceefc37abe1e"
sh % "hg heads -q" == "11:83ed350dc2d6"
sh % "hg status -C" == r"""
    A b.rename/b
      b/b
    A c.copy
      c
    A foo/foo
    R b/b
    ? a/a.orig"""

# There should be no shelves left
sh % "hg shelve -l"

if feature.check(["execbit"]):
    # Ensure that metadata-only changes are shelved
    os.chmod("a/a", 0o777)
    sh % "hg shelve -q -n execbit a/a"
    sh % "hg status a/a"
    sh % "hg unshelve -q execbit"
    sh % "hg status a/a" == "M a/a"
    sh % "hg revert a/a"


if feature.check(["symlink"]):
    # Ensure symlinks are properly handled
    sh % "rm a/a"
    sh % "ln -s foo a/a"
    sh % "hg shelve -q -n symlink a/a"
    sh % "hg status a/a"
    sh % "hg unshelve -q symlink"
    sh % "hg status a/a" == "M a/a"
    sh % "hg revert a/a"


# Set up another conflict between a commit and a shelved change
sh % "hg revert -q -C -a"
sh % "rm a/a.orig b.rename/b c.copy"
sh % "echo a" >> "a/a"
sh % "hg shelve -q"
sh % "echo x" >> "a/a"
sh % "hg ci -m 'create conflict'"
sh % "hg add foo/foo"

# If we resolve a conflict while unshelving, the unshelve should succeed
sh % "hg unshelve --tool ':merge-other' --keep" == r"""
    unshelving change 'default'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing .* "shelve changes to: second" (re)
    merging a/a"""
sh % "hg shelve -l" == "default * shelve changes to: second (glob)"
sh % "hg status" == r"""
    M a/a
    A foo/foo"""
sh % "cat a/a" == r"""
    a
    c
    a"""
sh % "cat" << r"""
a
c
x
""" > "a/a"
sh % "'HGMERGE=true' hg unshelve" == r"""
    unshelving change 'default'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing .* "shelve changes to: second" (re)
    merging a/a
    note: rebase of 18:056f8c92b111 created no changes to commit"""
sh % "hg shelve -l"
sh % "hg status" == "A foo/foo"
sh % "cat a/a" == r"""
    a
    c
    x"""

# Test keep and cleanup
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg shelve --list" == "default * shelve changes to: create conflict (glob)"
sh % "hg unshelve -k" == "unshelving change 'default'"
sh % "hg shelve --list" == "default * shelve changes to: create conflict (glob)"
sh % "hg shelve --cleanup"
sh % "hg shelve --list"

# Test bookmarks
sh % "hg bookmark test"
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg shelve" == r"""
    shelved as test
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg unshelve" == "unshelving change 'test'"
sh % "hg bookmark" == " * test                      * (glob)"

# Shelve should still work even if mq is disabled
sh % "hg --config 'extensions.mq=!' shelve" == r"""
    shelved as test
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg --config 'extensions.mq=!' shelve --list" == "test * shelve changes to: create conflict (glob)"
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg --config 'extensions.mq=!' unshelve" == "unshelving change 'test'"
sh % "hg bookmark" == " * test                      * (glob)"
sh % "cd .."

# Shelve should leave dirstate clean (issue4055)
sh % "hg init obsshelverebase"
sh % "cd obsshelverebase"
sh % "printf 'x\\ny\\n'" > "x"
sh % "echo z" > "z"
sh % "hg commit -Aqm xy"
sh % "echo z" >> "x"
sh % "hg commit -Aqm z"
sh % "hg up 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "printf 'a\\nx\\ny\\nz\\n'" > "x"
sh % "hg commit -Aqm xyz"
sh % "echo c" >> "z"
sh % "hg shelve" == r"""
    shelved as default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg rebase -d 1 --config 'extensions.rebase='" == r"""
    rebasing 323bfa07f744 "xyz"
    merging x"""
sh % "hg unshelve" == r'''
    unshelving change 'default'
    rebasing shelved changes
    rebasing a2281b51947d "shelve changes to: xyz"'''
sh % "hg status" == "M z"
sh % "cd .."

# Shelve should only unshelve pending changes (issue4068)
sh % "hg init obssh-onlypendingchanges"
sh % "cd obssh-onlypendingchanges"
sh % "touch a"
sh % "hg ci -Aqm a"
sh % "touch b"
sh % "hg ci -Aqm b"
sh % "hg up -q 0"
sh % "touch c"
sh % "hg ci -Aqm c"
sh % "touch d"
sh % "hg add d"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg up -q 1"
sh % "hg unshelve" == r'''
    unshelving change 'default'
    rebasing shelved changes
    rebasing 7eac9d98447f "shelve changes to: c"'''
sh % "hg status" == "A d"

# Unshelve should work on an ancestor of the original commit
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg up 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg unshelve" == r'''
    unshelving change 'default'
    rebasing shelved changes
    rebasing 325b64d70042 "shelve changes to: b"'''
sh % "hg status" == "A d"

# Test bug 4073 we need to enable obsolete markers for it
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
arg = sh.hg("--debug", "id", "-i", "-r", "1")
sh % ("hg debugobsolete %s" % arg) == "obsoleted 1 changesets"
sh % "hg unshelve" == "unshelving change 'default'"

# Unshelve should leave unknown files alone (issue4113)
sh % "echo e" > "e"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg status" == "? e"
sh % "hg unshelve" == "unshelving change 'default'"
sh % "hg status" == r"""
    A d
    ? e"""
sh % "cat e" == "e"

# 139. Unshelve should keep a copy of unknown files

sh % "hg add e"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 2 files removed, 0 files unresolved"""
sh % "echo z" > "e"
sh % "hg unshelve" == "unshelving change 'default'"
sh % "cat e" == "e"
sh % "cat e.orig" == "z"

# 140. Unshelve and conflicts with tracked and untracked files

#  preparing:

sh % "rm 'e.orig'"
sh % "hg ci -qm 'commit stuff'"
sh % "hg phase -p 'null:'"

#  no other changes - no merge:

sh % "echo f" > "f"
sh % "hg add f"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "echo g" > "f"
sh % "hg unshelve" == "unshelving change 'default'"
sh % "hg st" == r"""
    A f
    ? f.orig"""
sh % "cat f" == "f"
sh % "cat f.orig" == "g"

#  other uncommitted changes - merge:

sh % "hg st" == r"""
    A f
    ? f.orig"""
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg log -G --template '{rev}  {desc|firstline}  {author}'" == r"""
    @  9  commit stuff  test
    |
    | o  2  c  test
    |/
    o  0  a  test"""
sh % "mv f.orig f"
sh % "echo 1" > "a"
sh % "hg unshelve --date '1073741824 0'" == r"""
    unshelving change 'default'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing a0cc43106cdd "shelve changes to: commit stuff"
    merging f
    warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""
sh % "hg parents -T '{desc|firstline}\\n'" == r"""
    pending changes temporary commit
    shelve changes to: commit stuff"""

sh % "hg st" == r"""
    M f
    ? f.orig"""
sh % "cat f" == r"""
    <<<<<<< dest:   f53a8a3b0fad - test: pending changes temporary commit
    g
    =======
    f
    >>>>>>> source: a0cc43106cdd - test: shelve changes to: commit stuff"""
sh % "cat f.orig" == "g"
sh % "hg unshelve --abort -t false" == r"""
    tool option will be ignored
    rebase aborted
    unshelve of 'default' aborted"""
sh % "hg st" == r"""
    M a
    ? f.orig"""
sh % "cat f.orig" == "g"
sh % "hg unshelve" == r'''
    unshelving change 'default'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing a0cc43106cdd "shelve changes to: commit stuff"'''
sh % "hg st" == r"""
    M a
    A f
    ? f.orig"""

#  other committed changes - merge:

sh % "hg shelve f" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg ci a -m 'intermediate other change'"
sh % "mv f.orig f"
sh % "hg unshelve" == r"""
    unshelving change 'default'
    rebasing shelved changes
    rebasing a0cc43106cdd "shelve changes to: commit stuff"
    merging f
    warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""
sh % "hg st" == r"""
    M f
    ? f.orig"""
sh % "cat f" == r"""
    <<<<<<< dest:   * - test: intermediate other change (glob)
    g
    =======
    f
    >>>>>>> source: a0cc43106cdd - test: shelve changes to: commit stuff"""
sh % "cat f.orig" == "g"
sh % "hg unshelve --abort" == r"""
    rebase aborted
    unshelve of 'default' aborted"""
sh % "hg st" == "? f.orig"
sh % "cat f.orig" == "g"
sh % "hg shelve --delete default"

# Recreate some conflict again
sh % "cd ../obsrepo"
sh % "hg up -C -r 'test^'" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark test)"""
sh % "echo y" >> "a/a"
sh % "hg shelve" == r"""
    shelved as default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg up test" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (activating bookmark test)"""
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg unshelve" == r"""
    unshelving change 'default'
    rebasing shelved changes
    rebasing * "shelve changes to: second" (glob)
    merging a/a
    warning: 1 conflicts while merging a/a! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""
sh % "hg bookmark" == "   test                      * (glob)"

# Test that resolving all conflicts in one direction (so that the rebase
# is a no-op), works (issue4398)

sh % "hg revert -a -r ." == "reverting a/a (glob)"
sh % "hg resolve -m a/a" == r"""
    (no more unresolved files)
    continue: hg unshelve --continue"""
sh % "hg unshelve -c" == r"""
    rebasing * "shelve changes to: second" (glob)
    note: rebase of 23:f3b9a2b33e15 created no changes to commit
    unshelve of 'default' complete"""
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg diff"
sh % "hg status" == r"""
    ? a/a.orig
    ? foo/foo"""

sh % "hg shelve --delete --stat" == r"""
    abort: options '--delete' and '--stat' may not be used together
    [255]"""
sh % "hg shelve --delete --name NAME" == r"""
    abort: options '--delete' and '--name' may not be used together
    [255]"""

# Test interactive shelve
sh % "cat" << r"""
[ui]
interactive = true
""" >> "$HGRCPATH"
sh % "echo a" >> "a/b"
sh % "cat a/a" >> "a/b"
sh % "echo x" >> "a/b"
sh % "mv a/b a/a"
sh % "echo a" >> "foo/foo"
sh % "hg st" == r"""
    M a/a
    ? a/a.orig
    ? foo/foo"""
sh % "cat a/a" == r"""
    a
    a
    c
    x
    x"""
sh % "cat foo/foo" == r"""
    foo
    a"""
sh % "hg shelve --interactive --config 'ui.interactive=false'" == r"""
    abort: running non-interactively
    [255]"""
sh % "hg shelve --interactive" << r"""
y
y
n
""" == r"""
    diff --git a/a/a b/a/a
    2 hunks, 2 lines changed
    examine changes to 'a/a'? [Ynesfdaq?] y

    @@ -1,3 +1,4 @@
     a
    +a
     c
     x
    record change 1/2 to 'a/a'? [Ynesfdaq?] y

    @@ -2,2 +3,3 @@
     c
     x
    +x
    record change 2/2 to 'a/a'? [Ynesfdaq?] n

    shelved as test
    merging a/a
    0 files updated, 1 files merged, 0 files removed, 0 files unresolved"""
sh % "cat a/a" == r"""
    a
    c
    x
    x"""
sh % "cat foo/foo" == r"""
    foo
    a"""
sh % "hg st" == r"""
    M a/a
    ? foo/foo"""
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg log -r . -T '{desc|firstline}\\n'" == "create conflict"
sh % "hg unshelve" == r"""
    unshelving change 'test'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing * "shelve changes to: create conflict" (glob)
    merging a/a"""
sh % "hg bookmark" == " * test                      * (glob)"
sh % "hg log -r . -T '{desc|firstline}\\n'" == "create conflict"
sh % "cat a/a" == r"""
    a
    a
    c
    x
    x"""

# Shelve --patch and shelve --stat should work with a single valid shelfname
sh % "hg up --clean ." == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark test)"""
sh % "hg shelve --list"
sh % "echo 'patch a'" > "shelf-patch-a"
sh % "hg add shelf-patch-a"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "echo 'patch b'" > "shelf-patch-b"
sh % "hg add shelf-patch-b"
sh % "hg shelve" == r"""
    shelved as default-01
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg shelve --patch default default-01" == r"""
    default-01 * shelve changes to: create conflict (glob)

    diff --git a/shelf-patch-b b/shelf-patch-b
    new file mode 100644
    --- /dev/null
    +++ b/shelf-patch-b
    @@ -0,0 +1,1 @@
    +patch b
    default * shelve changes to: create conflict (glob)

    diff --git a/shelf-patch-a b/shelf-patch-a
    new file mode 100644
    --- /dev/null
    +++ b/shelf-patch-a
    @@ -0,0 +1,1 @@
    +patch a"""
sh % "hg shelve --stat default default-01" == r"""
    default-01 * shelve changes to: create conflict (glob)
     shelf-patch-b |  1 +
     1 files changed, 1 insertions(+), 0 deletions(-)
    default * shelve changes to: create conflict (glob)
     shelf-patch-a |  1 +
     1 files changed, 1 insertions(+), 0 deletions(-)"""
sh % "hg shelve --patch default" == r"""
    default * shelve changes to: create conflict (glob)

    diff --git a/shelf-patch-a b/shelf-patch-a
    new file mode 100644
    --- /dev/null
    +++ b/shelf-patch-a
    @@ -0,0 +1,1 @@
    +patch a"""
# No-argument --patch should also work
sh % "hg shelve --patch" == r"""
    default-01      (*s ago)    shelve changes to: create conflict (glob)

    diff --git a/shelf-patch-b b/shelf-patch-b
    new file mode 100644
    --- /dev/null
    +++ b/shelf-patch-b
    @@ -0,0 +1,1 @@
    +patch b"""
sh % "hg shelve --stat default" == r"""
    default * shelve changes to: create conflict (glob)
     shelf-patch-a |  1 +
     1 files changed, 1 insertions(+), 0 deletions(-)"""
sh % "hg shelve --patch nonexistentshelf" == r"""
    abort: cannot find shelf nonexistentshelf
    [255]"""
sh % "hg shelve --stat nonexistentshelf" == r"""
    abort: cannot find shelf nonexistentshelf
    [255]"""

# Test visibility of in-memory changes inside transaction to external hook
# ------------------------------------------------------------------------
sh % "echo xxxx" >> "x"
sh % "hg commit -m 'shelve changes to invoke rebase'"

# Unsupport by t.py: no external shell scripts
if feature.check("false"):
    sh % "hg bookmark unshelvedest"

    sh % "cat" << r"""
    echo "==== \$1:"
    hg parents --template "VISIBLE {node|short}\n"
    # test that pending changes are hidden
    unset HG_PENDING
    unset HG_SHAREDPENDING
    hg parents --template "ACTUAL  {node|short}\n"
    echo "===="
    """ > "$TESTTMP/checkvisibility.sh"

    sh % "cat" << r"""
    [defaults]
    # to fix hash id of temporary revisions
    unshelve = --date '0 0'
    """ >> ".hg/hgrc"

    # "hg unshelve"implies steps below:
    # (1) commit changes in the working directory
    # (2) note shelved revision
    # (3) rebase: merge shelved revision into temporary wc changes
    # (4) rebase: commit merged revision
    # (5) rebase: update to a new commit
    # (6) update to original working copy parent

    # == test visibility to external preupdate hook

    sh % "cat" << r"""
    [hooks]
    preupdate.visibility = sh $TESTTMP/checkvisibility.sh preupdate
    """ >> ".hg/hgrc"

    sh % "echo nnnn" >> "n"

    sh % 'sh "$TESTTMP/checkvisibility.sh" before-unshelving' == r"""
        ==== before-unshelving:
        VISIBLE 47f190a8b2e0
        ACTUAL  47f190a8b2e0
        ===="""

    sh % "hg unshelve --keep default" == r"""
        temporarily committing pending changes (restore with 'hg unshelve --abort')
        rebasing shelved changes
        rebasing 27:80096f006bb2 "shelve changes to: create conflict"
        ==== preupdate:
        VISIBLE (?!f77bf047d4c5).* (re)
        ACTUAL  47f190a8b2e0
        ====
        ==== preupdate:
        VISIBLE (?!f77bf047d4c5).* (re)
        ACTUAL  47f190a8b2e0
        ====
        ==== preupdate:
        VISIBLE (?!f77bf047d4c5).* (re)
        ACTUAL  47f190a8b2e0
        ===="""

    sh % "cat" << r"""
    [hooks]
    preupdate.visibility =
    """ >> ".hg/hgrc"

    sh % 'sh "$TESTTMP/checkvisibility.sh" after-unshelving' == r"""
        ==== after-unshelving:
        VISIBLE 47f190a8b2e0
        ACTUAL  47f190a8b2e0
        ===="""

    # == test visibility to external update hook

    sh % "hg update -q -C unshelvedest"

    sh % "cat" << r"""
    [hooks]
    update.visibility = sh $TESTTMP/checkvisibility.sh update
    """ >> ".hg/hgrc"

    sh % "echo nnnn" >> "n"

    sh % 'sh "$TESTTMP/checkvisibility.sh" before-unshelving' == r"""
        ==== before-unshelving:
        VISIBLE 47f190a8b2e0
        ACTUAL  47f190a8b2e0
        ===="""

    sh % "hg unshelve --keep default" == r"""
        temporarily committing pending changes (restore with 'hg unshelve --abort')
        rebasing shelved changes
        rebasing 27:80096f006bb2 "shelve changes to: create conflict"
        ==== update:
        VISIBLE f08f4865d656
        VISIBLE 80096f006bb2
        ACTUAL  47f190a8b2e0
        ====
        ==== update:
        VISIBLE f08f4865d656
        ACTUAL  47f190a8b2e0
        ====
        ==== update:
        VISIBLE 47f190a8b2e0
        ACTUAL  47f190a8b2e0
        ===="""

    sh % "cat" << r"""
    [hooks]
    update.visibility =
    """ >> ".hg/hgrc"

    sh % 'sh "$TESTTMP/checkvisibility.sh" after-unshelving' == r"""
        ==== after-unshelving:
        VISIBLE 47f190a8b2e0
        ACTUAL  47f190a8b2e0
        ===="""
    sh % "hg bookmark -d unshelvedest"
    sh % "cd .."

# Test .orig files go where the user wants them to
# ---------------------------------------------------------------
sh % "newrepo obssh-salvage"
sh % "echo content" > "root"
sh % "hg commit -A -m root -q"
sh % "echo ''" > "root"
sh % "hg shelve -q"
sh % "echo contADDent" > "root"
sh % "hg unshelve -q --config 'ui.origbackuppath=.hg/origbackups'" == r"""
    warning: 1 conflicts while merging root! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""
sh % "ls .hg/origbackups" == "root"
sh % "rm -rf .hg/origbackups"

# Test Abort unshelve always gets user out of the unshelved state
# ---------------------------------------------------------------
# Wreak havoc on the unshelve process
sh % "rm .hg/unshelverebasestate"
sh % "hg unshelve --abort" == r"""
    unshelve of 'default' aborted
    abort: $ENOENT$
    [255]"""
# Can the user leave the current state?
sh % "hg up -C ." == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Try again but with a corrupted shelve state file
sh % "hg up -r 0 -q"
sh % "echo ''" > "root"
sh % "hg shelve -q"
sh % "echo contADDent" > "root"
sh % "hg unshelve -q" == r"""
    warning: 1 conflicts while merging root! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""

with open(".hg/histedit-state", "w") as f:
    f.write(open(".hg/shelvedstate").read().replace("ae8c668541e8", "123456789012"))

sh % "hg unshelve --abort" | sh % "head -1" == "rebase aborted"
sh % "hg up -C ." == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "cd .."

# Keep active bookmark while (un)shelving even on shared repo (issue4940)
# -----------------------------------------------------------------------
sh % "cat" << r"""
[extensions]
share =
[experimnetal]
evolution=createmarkers
""" >> "$HGRCPATH"
sh % "hg bookmarks -R obsrepo" == "   test                      19:a72d63c69876"
sh % "hg share -B obsrepo obsshare" == r"""
    updating working directory
    6 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd obsshare"

sh % "hg bookmarks" == "   test                      19:a72d63c69876"
sh % "hg bookmarks foo"
sh % "hg bookmarks" == r"""
     * foo                       29:47f190a8b2e0
       test                      19:a72d63c69876"""
sh % "echo x" >> "x"
sh % "hg shelve" == r"""
    shelved as foo
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bookmarks" == r"""
     * foo                       29:47f190a8b2e0
       test                      19:a72d63c69876"""

sh % "hg unshelve" == "unshelving change 'foo'"
sh % "hg bookmarks" == r"""
     * foo                       29:47f190a8b2e0
       test                      19:a72d63c69876"""

sh % "cd .."

# Shelve and unshelve unknown files. For the purposes of unshelve, a shelved
# unknown file is the same as a shelved added file, except that it will be in
# unknown state after unshelve if and only if it was either absent or unknown
# before the unshelve operation.
sh % "hg init obssh-unknowns"
sh % "cd obssh-unknowns"

# The simplest case is if I simply have an unknown file that I shelve and unshelve
sh % "echo unknown" > "unknown"
sh % "hg status" == "? unknown"
sh % "hg shelve --unknown" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg status"
sh % "hg unshelve" == "unshelving change 'default'"
sh % "hg status" == "? unknown"
sh % "rm unknown"

# If I shelve, add the file, and unshelve, does it stay added?
sh % "echo unknown" > "unknown"
sh % "hg shelve -u" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg status"
sh % "touch unknown"
sh % "hg add unknown"
sh % "hg status" == "A unknown"
sh % "hg unshelve" == r"""
    unshelving change 'default'
    temporarily committing pending changes (restore with 'hg unshelve --abort')
    rebasing shelved changes
    rebasing c850bce25d9f "(changes in empty repository)"
    merging unknown"""
sh % "hg status" == "A unknown"
sh % "hg forget unknown"
sh % "rm unknown"

# And if I shelve, commit, then unshelve, does it become modified?
sh % "echo unknown" > "unknown"
sh % "hg shelve -u" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "hg status"
sh % "touch unknown"
sh % "hg add unknown"
sh % "hg commit -qm 'Add unknown'"
sh % "hg status"
sh % "hg unshelve" == r"""
    unshelving change 'default'
    rebasing shelved changes
    rebasing c850bce25d9f "(changes in empty repository)"
    merging unknown"""
sh % "hg status" == "M unknown"
sh % "hg remove --force unknown"
sh % "hg commit -qm 'Remove unknown'"
sh % "cd .."

# Prepare unshelve with a corrupted shelvedstate
sh % "hg init obssh-r1"
sh % "cd obssh-r1"
sh % "echo text1" > "file"
sh % "hg add file"
sh % "hg shelve" == r"""
    shelved as default
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved"""
sh % "echo text2" > "file"
sh % "hg ci -Am text1" == "adding file"
sh % "hg unshelve" == r"""
    unshelving change 'default'
    rebasing shelved changes
    rebasing a6a994ce5ac2 "(changes in empty repository)"
    merging file
    warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""
sh % "echo somethingsomething" > ".hg/shelvedstate"

# Unshelve --continue fails with appropriate message if shelvedstate is corrupted
sh % "hg continue" == r"""
    abort: corrupted shelved state file
    (please run hg unshelve --abort to abort unshelve operation)
    [255]"""

# Unshelve --abort works with a corrupted shelvedstate
sh % "hg unshelve --abort" == r"""
    could not read shelved state file, your working copy may be in an unexpected state
    please update to some commit"""

# Unshelve --abort fails with appropriate message if there's no unshelve in
# progress
sh % "hg unshelve --abort" == r"""
    abort: no unshelve in progress
    [255]"""
sh % "cd .."

# Unshelve respects --keep even if user intervention is needed
sh % "hg init obs-unshelvekeep"
sh % "cd obs-unshelvekeep"
sh % "echo 1" > "file"
sh % "hg ci -Am 1" == "adding file"
sh % "echo 2" >> "file"
sh % "hg shelve" == r"""
    shelved as default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "echo 3" > "file"
sh % "hg ci -Am 13"
sh % "hg shelve --list" == "default * shelve changes to: 1 (glob)"
sh % "hg unshelve --keep" == r"""
    unshelving change 'default'
    rebasing shelved changes
    rebasing 49351a7ca591 "shelve changes to: 1"
    merging file
    warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
    [1]"""
sh % "hg resolve --mark file" == r"""
    (no more unresolved files)
    continue: hg unshelve --continue"""
sh % "hg unshelve --continue" == r"""
    rebasing 49351a7ca591 "shelve changes to: 1"
    unshelve of 'default' complete"""
sh % "hg shelve --list" == "default * shelve changes to: 1 (glob)"
sh % "cd .."

# Unshelving a stripped commit aborts with an explanatory message
sh % "hg init obs-unshelve-stripped-commit"
sh % "cd obs-unshelve-stripped-commit"
sh % "echo 1" > "file"
sh % "hg ci -Am 1" == "adding file"
sh % "echo 2" >> "file"
sh % "hg shelve" == r"""
    shelved as default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg debugstrip -r 1 --config 'experimental.evolution=!' --hidden" == r"""
    obsolete feature not enabled but 1 markers found!
    saved backup bundle to .* (re)"""
sh % "hg unshelve" == r"""
    unshelving change 'default'
    abort: shelved node 49351a7ca59142b32064896a48f50bdecccf8ea0 not found in repo
    [255]"""
sh % "cd .."

# Test revsetpredicate 'shelved'
# For this test enabled shelve extension is enough, and it is enabled at the top of the file
sh % "hg init test-log-shelved"
sh % "cd test-log-shelved"


def testshelvedcount(n):
    sh % 'hg log --hidden -r "shelved()" --template "."' == "." * int(n)


shlib.__dict__["testshelvedcount"] = testshelvedcount

sh % "touch file1"
sh % "touch file2"
sh % "touch file3"
sh % "hg addremove" == r"""
    adding file1
    adding file2
    adding file3"""
sh % "hg commit -m 'Add test files'"
sh % "echo 1" >> "file1"
sh % "hg shelve" == r"""
    shelved as default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "testshelvedcount 1"
sh % "echo 2" >> "file2"
sh % "hg shelve" == r"""
    shelved as default-01
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "testshelvedcount 2"
sh % "echo 3" >> "file3"
sh % "hg shelve" == r"""
    shelved as default-02
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "testshelvedcount 3"
sh % "hg log --hidden -r 'shelved()' --template '{node|short} {shelvename}\\n'" == r"""
    d7a61836580c default
    9dcce8f0ff7d default-01
    225e1bca0190 default-02"""
sh % "hg unshelve" > "/dev/null"
sh % "testshelvedcount 2"
sh % "hg unshelve" > "/dev/null"
sh % "testshelvedcount 1"
sh % "hg unshelve" > "/dev/null"
sh % "testshelvedcount 0"
sh % "cd .."

# Test interrupted shelve - this should not lose work

sh % "newrepo"
sh % "echo 1" > "file1"
sh % "echo 1" > "file2"
sh % "hg commit -Aqm commit1"
sh % "echo 2" > "file2"


def createmarkers(orig, *args, **kwargs):
    orig(*args, **kwargs)
    raise KeyboardInterrupt


with extensions.wrappedfunction(obsolete, "createmarkers", createmarkers):
    sh % "hg shelve" == r"""
        transaction abort!
        rollback completed
        interrupted!
        [255]"""

sh % "cat file2" == "2"
sh % "tglog" == "@  0: 6408d34d8180 'commit1'"


def update(orig, repo, *args, **kwargs):
    if repo.ui.configbool("abortupdate", "after"):
        orig(repo, *args, **kwargs)
    raise KeyboardInterrupt


with extensions.wrappedfunction(hg, "update", update):
    sh % "hg shelve" == r"""
        shelved as default
        interrupted!
        [255]"""

sh % "cat file2" == "2"
sh % "tglog" == "@  0: 6408d34d8180 'commit1'"
sh % "hg update --clean --quiet ."
sh % "hg shelve --list" == "default * shelve changes to: commit1 (glob)"
sh % "hg unshelve" == "unshelving change 'default'"
sh % "cat file2" == "2"
with extensions.wrappedfunction(hg, "update", update):
    sh % "hg shelve --config 'abortupdate.after=true'" == r"""
        shelved as default
        1 files updated, 0 files merged, 0 files removed, 0 files unresolved
        interrupted!
        [255]"""
sh % "cat file2" == "1"
sh % "tglog" == "@  0: 6408d34d8180 'commit1'"
sh % "hg shelve --list" == "default * shelve changes to: commit1 (glob)"
sh % 'hg log --hidden -r tip -T \'{node|short} "{shelvename}" "{desc}"\\n\'' == 'f70d92a087e8 "default" "shelve changes to: commit1"'
sh % "hg unshelve" == "unshelving change 'default'"
sh % "cat file2" == "2"
