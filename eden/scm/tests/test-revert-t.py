# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

import generateworkingcopystates
from testutil.autofix import eq
from testutil.dott import feature, sh, testtmp  # noqa: F401


def dircontent():
    # generate a simple text view of the directory for easy comparison
    files = os.listdir(".")
    files.sort()
    output = []
    for filename in files:
        if os.path.isdir(filename):
            continue
        content = open(filename).read()
        output.append("%-6s %s" % (content.strip(), filename))
    return "\n".join(output)


sh % ". helpers-usechg.sh"

sh % "hg init repo"
sh % "cd repo"
sh % "echo 123" > "a"
sh % "echo 123" > "c"
sh % "echo 123" > "e"
sh % "hg add a c e"
sh % "hg commit -m first a c e"

# nothing changed

sh % "hg revert" == r"""
    abort: no files or directories specified
    (use --all to revert all files)
    [255]"""
sh % "hg revert --all"

# Introduce some changes and revert them
# --------------------------------------

sh % "echo 123" > "b"

sh % "hg status" == "? b"
sh % "echo 12" > "c"

sh % "hg status" == r"""
    M c
    ? b"""
sh % "hg add b"

sh % "hg status" == r"""
    M c
    A b"""
sh % "hg rm a"

sh % "hg status" == r"""
    M c
    A b
    R a"""

# revert removal of a file

sh % "hg revert a"
sh % "hg status" == r"""
    M c
    A b"""

# revert addition of a file

sh % "hg revert b"
sh % "hg status" == r"""
    M c
    ? b"""

# revert modification of a file (--no-backup)

sh % "hg revert --no-backup c"
sh % "hg status" == "? b"

# revert deletion (! status) of a added file
# ------------------------------------------

sh % "hg add b"

sh % "hg status b" == "A b"
sh % "rm b"
sh % "hg status b" == "! b"
sh % "hg revert -v b" == "forgetting b"
sh % "hg status b" == "b: * (glob)"

sh % "ls" == r"""
    a
    c
    e"""

# Test creation of backup (.orig) files
# -------------------------------------

sh % "echo z" > "e"
sh % "hg revert --all -v" == r"""
    saving current version of e as e.orig
    reverting e"""

# Test creation of backup (.orig) file in configured file location
# ----------------------------------------------------------------

sh % "echo z" > "e"
sh % "hg revert --all -v --config 'ui.origbackuppath=.hg/origbackups'" == r"""
    creating directory: $TESTTMP/repo/.hg/origbackups
    saving current version of e as $TESTTMP/repo/.hg/origbackups/e
    reverting e"""
sh % "rm -rf .hg/origbackups"

# revert on clean file (no change)
# --------------------------------

sh % "hg revert a" == "no changes needed to a"

# revert on an untracked file
# ---------------------------

sh % "echo q" > "q"
sh % "hg revert q" == "file not managed: q"
sh % "rm q"

# revert on file that does not exists
# -----------------------------------

sh % "hg revert notfound" == "notfound: no such file in rev 334a9e57682c"
sh % "touch d"
sh % "hg add d"
sh % "hg rm a"
sh % "hg commit -m second"
sh % "echo z" > "z"
sh % "hg add z"
sh % "hg st" == r"""
    A z
    ? e.orig"""

# revert to another revision (--rev)
# ----------------------------------

sh % "hg revert --all -r0" == r"""
    adding a
    removing d
    forgetting z"""

# revert explicitly to parent (--rev)
# -----------------------------------

sh % "hg revert --all -rtip" == r"""
    forgetting a
    undeleting d"""
sh % "rm a *.orig"

# revert to another revision (--rev) and exact match
# --------------------------------------------------

# exact match are more silent

sh % "hg revert -r0 a"
sh % "hg st a" == "A a"
sh % "hg rm d"
sh % "hg st d" == "R d"

# should keep d removed

sh % "hg revert -r0 d" == "no changes needed to d"
sh % "hg st d" == "R d"

sh % "hg update -C" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# revert of exec bit
# ------------------

if feature.check(["execbit"]):
    sh % "chmod +x c"
    sh % "hg revert --all" == "reverting c"

    sh % "test -x c" == "[1]"

    sh % "chmod +x c"
    sh % "hg commit -m exe"

    sh % "chmod -x c"
    sh % "hg revert --all" == "reverting c"

    sh % "test -x c"
    sh % "echo executable" == "executable"


# Test that files reverted to other than the parent are treated as
# "modified", even if none of mode, size and timestamp of it isn't
# changed on the filesystem (see also issue4583).

sh % "echo 321" > "e"
sh % "hg diff --git" == r"""
    diff --git a/e b/e
    --- a/e
    +++ b/e
    @@ -1,1 +1,1 @@
    -123
    +321"""
sh % "hg commit -m 'ambiguity from size'"

sh % "cat e" == "321"
sh % "touch -t 200001010000 e"
sh % "hg debugrebuildstate"

sh % "cat" << r"""
[fakedirstatewritetime]
# emulate invoking dirstate.write() via repo.status()
# at 2000-01-01 00:00
fakenow = 200001010000

[extensions]
fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
""" >> ".hg/hgrc"
sh % "hg revert -r 0 e"
sh % "cat" << r"""
[extensions]
fakedirstatewritetime = !
""" >> ".hg/hgrc"

sh % "cat e" == "123"
sh % "touch -t 200001010000 e"
sh % "hg status -A e" == "M e"

sh % "cd .."


# Issue241: update and revert produces inconsistent repositories
# --------------------------------------------------------------

sh % "hg init a"
sh % "cd a"
sh % "echo a" >> "a"
sh % "hg commit -A -d '1 0' -m a" == "adding a"
sh % "echo a" >> "a"
sh % "hg commit -d '2 0' -m a"
sh % "hg update 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "mkdir b"
sh % "echo b" > "b/b"

# call `hg revert` with no file specified
# ---------------------------------------

sh % "hg revert -rtip" == r"""
    abort: no files or directories specified
    (use --all to revert all files, or 'hg update 1' to update)
    [255]"""

# call `hg revert` with -I
# ---------------------------

sh % "echo a" >> "a"
sh % "hg revert -I a" == "reverting a"

# call `hg revert` with -X
# ---------------------------

sh % "echo a" >> "a"
sh % "hg revert -X d" == "reverting a"

# call `hg revert` with --all
# ---------------------------

sh % "hg revert --all -rtip" == "reverting a"
sh % "rm 'a.orig'"

# Issue332: confusing message when reverting directory
# ----------------------------------------------------

sh % "hg ci -A -m b" == "adding b/b"
sh % "echo foobar" > "b/b"
sh % "mkdir newdir"
sh % "echo foo" > "newdir/newfile"
sh % "hg add newdir/newfile"
sh % "hg revert b newdir" == r"""
    reverting b/b
    forgetting newdir/newfile"""
sh % "echo foobar" > "b/b"
sh % "hg revert ." == "reverting b/b"


# reverting a rename target should revert the source
# --------------------------------------------------

sh % "hg mv a newa"
sh % "hg revert newa"
sh % "hg st a newa" == "? newa"

# Also true for move overwriting an existing file

sh % "hg mv --force a b/b"
sh % "hg revert b/b"
sh % "hg status a b/b"

sh % "cd .."

sh % "hg init ignored"
sh % "cd ignored"
sh % "echo ignored" > ".gitignore"
sh % "echo ignoreddir" >> ".gitignore"
sh % "echo removed" >> ".gitignore"

sh % "mkdir ignoreddir"
sh % "touch ignoreddir/file"
sh % "touch ignoreddir/removed"
sh % "touch ignored"
sh % "touch removed"

# 4 ignored files (we will add/commit everything)

sh % "hg st -A -X .gitignore" == r"""
    I ignored
    I ignoreddir/file
    I ignoreddir/removed
    I removed"""
sh % "hg ci -qAm 'add files' ignored ignoreddir/file ignoreddir/removed removed"

sh % "echo" >> "ignored"
sh % "echo" >> "ignoreddir/file"
sh % "hg rm removed ignoreddir/removed"

# should revert ignored* and undelete *removed
# --------------------------------------------

sh % "hg revert -a --no-backup" == r"""
    reverting ignored
    reverting ignoreddir/file
    undeleting ignoreddir/removed
    undeleting removed"""
sh % "hg st -mardi"

sh % "hg up -qC"
sh % "echo" >> "ignored"
sh % "hg rm removed"

# should silently revert the named files
# --------------------------------------

sh % "hg revert --no-backup ignored removed"
sh % "hg st -mardi"

# Reverting copy (issue3920)
# --------------------------

# someone set up us the copies

sh % "rm .gitignore"
sh % "hg update -C" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg mv ignored allyour"
sh % "hg copy removed base"
sh % "hg commit -m rename"

# copies and renames, you have no chance to survive make your time (issue3920)

sh % "hg update '.^'" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "hg revert -rtip -a" == r"""
    adding allyour
    adding base
    removing ignored"""
sh % "hg status -C" == r"""
    A allyour
      ignored
    A base
      removed
    R ignored"""

# Test revert of a file added by one side of the merge
# ====================================================

# remove any pending change

sh % "hg revert --all" == r"""
    forgetting allyour
    forgetting base
    undeleting ignored"""
sh % "hg purge --all --config 'extensions.purge='"

# Adds a new commit

sh % "echo foo" > "newadd"
sh % "hg add newadd"
sh % "hg commit -m 'other adds'"


# merge it with the other head

sh % "hg merge" == r"""
    2 files updated, 0 files merged, 1 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg summary" == r"""
    parent: 2:b8ec310b2d4e 
     other adds
    parent: 1:f6180deb8fbe 
     rename
    commit: 2 modified, 1 removed (merge)
    phases: 3 draft"""

# clarifies who added what

sh % "hg status" == r"""
    M allyour
    M base
    R ignored"""
sh % "hg status --change 'p1()'" == "A newadd"
sh % "hg status --change 'p2()'" == r"""
    A allyour
    A base
    R ignored"""

# revert file added by p1() to p1() state
# -----------------------------------------

sh % "hg revert -r 'p1()' 'glob:newad?'"
sh % "hg status" == r"""
    M allyour
    M base
    R ignored"""

# revert file added by p1() to p2() state
# ------------------------------------------

sh % "hg revert -r 'p2()' 'glob:newad?'" == "removing newadd"
sh % "hg status" == r"""
    M allyour
    M base
    R ignored
    R newadd"""

# revert file added by p2() to p2() state
# ------------------------------------------

sh % "hg revert -r 'p2()' 'glob:allyou?'"
sh % "hg status" == r"""
    M allyour
    M base
    R ignored
    R newadd"""

# revert file added by p2() to p1() state
# ------------------------------------------

sh % "hg revert -r 'p1()' 'glob:allyou?'" == "removing allyour"
sh % "hg status" == r"""
    M base
    R allyour
    R ignored
    R newadd"""

# Systematic behavior validation of most possible cases
# =====================================================

# This section tests most of the possible combinations of revision states and
# working directory states. The number of possible cases is significant but they
# but they all have a slightly different handling. So this section commits to
# and testing all of them to allow safe refactoring of the revert code.

# A python script is used to generate a file history for each combination of
# states, on one side the content (or lack thereof) in two revisions, and
# on the other side, the content and "tracked-ness" of the working directory. The
# three states generated are:

# - a "base" revision
# - a "parent" revision
# - the working directory (based on "parent")

# The files generated have names of the form:

#  <rev1-content>_<rev2-content>_<working-copy-content>-<tracked-ness>

# All known states are not tested yet. See inline documentation for details.
# Special cases from merge and rename are not tested by this section.

# Write the python script to disk
# -------------------------------

# check list of planned files

eq(
    generateworkingcopystates.main("filelist", 2),
    r"""
    content1_content1_content1-tracked
    content1_content1_content1-untracked
    content1_content1_content3-tracked
    content1_content1_content3-untracked
    content1_content1_missing-tracked
    content1_content1_missing-untracked
    content1_content2_content1-tracked
    content1_content2_content1-untracked
    content1_content2_content2-tracked
    content1_content2_content2-untracked
    content1_content2_content3-tracked
    content1_content2_content3-untracked
    content1_content2_missing-tracked
    content1_content2_missing-untracked
    content1_missing_content1-tracked
    content1_missing_content1-untracked
    content1_missing_content3-tracked
    content1_missing_content3-untracked
    content1_missing_missing-tracked
    content1_missing_missing-untracked
    missing_content2_content2-tracked
    missing_content2_content2-untracked
    missing_content2_content3-tracked
    missing_content2_content3-untracked
    missing_content2_missing-tracked
    missing_content2_missing-untracked
    missing_missing_content3-tracked
    missing_missing_content3-untracked
    missing_missing_missing-tracked
    missing_missing_missing-untracked""",
)
# Script to make a simple text version of the content
# ---------------------------------------------------

# Generate appropriate repo state
# -------------------------------

sh % "hg init revert-ref"
sh % "cd revert-ref"

# Generate base changeset

generateworkingcopystates.main("state", 2, 1)

sh % "hg addremove --similarity 0" == r"""
    adding content1_content1_content1-tracked
    adding content1_content1_content1-untracked
    adding content1_content1_content3-tracked
    adding content1_content1_content3-untracked
    adding content1_content1_missing-tracked
    adding content1_content1_missing-untracked
    adding content1_content2_content1-tracked
    adding content1_content2_content1-untracked
    adding content1_content2_content2-tracked
    adding content1_content2_content2-untracked
    adding content1_content2_content3-tracked
    adding content1_content2_content3-untracked
    adding content1_content2_missing-tracked
    adding content1_content2_missing-untracked
    adding content1_missing_content1-tracked
    adding content1_missing_content1-untracked
    adding content1_missing_content3-tracked
    adding content1_missing_content3-untracked
    adding content1_missing_missing-tracked
    adding content1_missing_missing-untracked"""
sh % "hg status" == r"""
    A content1_content1_content1-tracked
    A content1_content1_content1-untracked
    A content1_content1_content3-tracked
    A content1_content1_content3-untracked
    A content1_content1_missing-tracked
    A content1_content1_missing-untracked
    A content1_content2_content1-tracked
    A content1_content2_content1-untracked
    A content1_content2_content2-tracked
    A content1_content2_content2-untracked
    A content1_content2_content3-tracked
    A content1_content2_content3-untracked
    A content1_content2_missing-tracked
    A content1_content2_missing-untracked
    A content1_missing_content1-tracked
    A content1_missing_content1-untracked
    A content1_missing_content3-tracked
    A content1_missing_content3-untracked
    A content1_missing_missing-tracked
    A content1_missing_missing-untracked"""
sh % "hg commit -m base"

# (create a simple text version of the content)

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content1 content1_content1_content3-untracked
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content1 content1_content2_content1-tracked
    content1 content1_content2_content1-untracked
    content1 content1_content2_content2-tracked
    content1 content1_content2_content2-untracked
    content1 content1_content2_content3-tracked
    content1 content1_content2_content3-untracked
    content1 content1_content2_missing-tracked
    content1 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content1 content1_missing_content3-tracked
    content1 content1_missing_content3-untracked
    content1 content1_missing_missing-tracked
    content1 content1_missing_missing-untracked""",
)

# Create parent changeset

generateworkingcopystates.main("state", 2, 2)
sh % "hg addremove --similarity 0" == r"""
    removing content1_missing_content1-tracked
    removing content1_missing_content1-untracked
    removing content1_missing_content3-tracked
    removing content1_missing_content3-untracked
    removing content1_missing_missing-tracked
    removing content1_missing_missing-untracked
    adding missing_content2_content2-tracked
    adding missing_content2_content2-untracked
    adding missing_content2_content3-tracked
    adding missing_content2_content3-untracked
    adding missing_content2_missing-tracked
    adding missing_content2_missing-untracked"""
sh % "hg status" == r"""
    M content1_content2_content1-tracked
    M content1_content2_content1-untracked
    M content1_content2_content2-tracked
    M content1_content2_content2-untracked
    M content1_content2_content3-tracked
    M content1_content2_content3-untracked
    M content1_content2_missing-tracked
    M content1_content2_missing-untracked
    A missing_content2_content2-tracked
    A missing_content2_content2-untracked
    A missing_content2_content3-tracked
    A missing_content2_content3-untracked
    A missing_content2_missing-tracked
    A missing_content2_missing-untracked
    R content1_missing_content1-tracked
    R content1_missing_content1-untracked
    R content1_missing_content3-tracked
    R content1_missing_content3-untracked
    R content1_missing_missing-tracked
    R content1_missing_missing-untracked"""
sh % "hg commit -m parent"

# (create a simple text version of the content)

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content1 content1_content1_content3-untracked
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content2 content1_content2_content1-tracked
    content2 content1_content2_content1-untracked
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content2 content1_content2_content3-tracked
    content2 content1_content2_content3-untracked
    content2 content1_content2_missing-tracked
    content2 content1_content2_missing-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content2 missing_content2_content3-tracked
    content2 missing_content2_content3-untracked
    content2 missing_content2_missing-tracked
    content2 missing_content2_missing-untracked""",
)

# Setup working directory

generateworkingcopystates.main("state", 2, "wc")

sh % "hg addremove --similarity 0" == r"""
    adding content1_missing_content1-tracked
    adding content1_missing_content1-untracked
    adding content1_missing_content3-tracked
    adding content1_missing_content3-untracked
    adding content1_missing_missing-tracked
    adding content1_missing_missing-untracked
    adding missing_missing_content3-tracked
    adding missing_missing_content3-untracked
    adding missing_missing_missing-tracked
    adding missing_missing_missing-untracked"""
sh % "hg forget *_*_*-untracked"
sh % "rm *_*_missing-*"
sh % "hg status" == r"""
    M content1_content1_content3-tracked
    M content1_content2_content1-tracked
    M content1_content2_content3-tracked
    M missing_content2_content3-tracked
    A content1_missing_content1-tracked
    A content1_missing_content3-tracked
    A missing_missing_content3-tracked
    R content1_content1_content1-untracked
    R content1_content1_content3-untracked
    R content1_content1_missing-untracked
    R content1_content2_content1-untracked
    R content1_content2_content2-untracked
    R content1_content2_content3-untracked
    R content1_content2_missing-untracked
    R missing_content2_content2-untracked
    R missing_content2_content3-untracked
    R missing_content2_missing-untracked
    ! content1_content1_missing-tracked
    ! content1_content2_missing-tracked
    ! content1_missing_missing-tracked
    ! missing_content2_missing-tracked
    ! missing_missing_missing-tracked
    ? content1_missing_content1-untracked
    ? content1_missing_content3-untracked
    ? missing_missing_content3-untracked"""

sh % "hg status --rev 'desc(\"base\")'" == r"""
    M content1_content1_content3-tracked
    M content1_content2_content2-tracked
    M content1_content2_content3-tracked
    M content1_missing_content3-tracked
    A missing_content2_content2-tracked
    A missing_content2_content3-tracked
    A missing_missing_content3-tracked
    R content1_content1_content1-untracked
    R content1_content1_content3-untracked
    R content1_content1_missing-untracked
    R content1_content2_content1-untracked
    R content1_content2_content2-untracked
    R content1_content2_content3-untracked
    R content1_content2_missing-untracked
    R content1_missing_content1-untracked
    R content1_missing_content3-untracked
    R content1_missing_missing-untracked
    ! content1_content1_missing-tracked
    ! content1_content2_missing-tracked
    ! content1_missing_missing-tracked
    ! missing_content2_missing-tracked
    ! missing_missing_missing-tracked
    ? missing_missing_content3-untracked"""

# (create a simple text version of the content)

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content3 content1_content1_content3-tracked
    content3 content1_content1_content3-untracked
    content1 content1_content2_content1-tracked
    content1 content1_content2_content1-untracked
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content3 content1_content2_content3-tracked
    content3 content1_content2_content3-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content3 content1_missing_content3-tracked
    content3 content1_missing_content3-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content3 missing_content2_content3-tracked
    content3 missing_content2_content3-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)

sh % "cd .."

# Test revert --all to parent content
# -----------------------------------

# (setup from reference repo)

sh % "cp -R revert-ref revert-parent-all"
sh % "cd revert-parent-all"

# check revert output

sh % "hg revert --all" == r"""
    undeleting content1_content1_content1-untracked
    reverting content1_content1_content3-tracked
    undeleting content1_content1_content3-untracked
    reverting content1_content1_missing-tracked
    undeleting content1_content1_missing-untracked
    reverting content1_content2_content1-tracked
    undeleting content1_content2_content1-untracked
    undeleting content1_content2_content2-untracked
    reverting content1_content2_content3-tracked
    undeleting content1_content2_content3-untracked
    reverting content1_content2_missing-tracked
    undeleting content1_content2_missing-untracked
    forgetting content1_missing_content1-tracked
    forgetting content1_missing_content3-tracked
    forgetting content1_missing_missing-tracked
    undeleting missing_content2_content2-untracked
    reverting missing_content2_content3-tracked
    undeleting missing_content2_content3-untracked
    reverting missing_content2_missing-tracked
    undeleting missing_content2_missing-untracked
    forgetting missing_missing_content3-tracked
    forgetting missing_missing_missing-tracked"""

# Compare resulting directory with revert target.

# The diff is filtered to include change only. The only difference should be
# additional `.orig` backup file when applicable.

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content3 content1_content1_content3-tracked.orig
    content1 content1_content1_content3-untracked
    content3 content1_content1_content3-untracked.orig
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content2 content1_content2_content1-tracked
    content1 content1_content2_content1-tracked.orig
    content2 content1_content2_content1-untracked
    content1 content1_content2_content1-untracked.orig
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content2 content1_content2_content3-tracked
    content3 content1_content2_content3-tracked.orig
    content2 content1_content2_content3-untracked
    content3 content1_content2_content3-untracked.orig
    content2 content1_content2_missing-tracked
    content2 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content3 content1_missing_content3-tracked
    content3 content1_missing_content3-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content2 missing_content2_content3-tracked
    content3 missing_content2_content3-tracked.orig
    content2 missing_content2_content3-untracked
    content3 missing_content2_content3-untracked.orig
    content2 missing_content2_missing-tracked
    content2 missing_content2_missing-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)
sh % "cd .."

# Test revert --all to "base" content
# -----------------------------------

# (setup from reference repo)

sh % "cp -R revert-ref revert-base-all"
sh % "cd revert-base-all"

# check revert output

sh % "hg revert --all --rev 'desc(base)'" == r"""
    undeleting content1_content1_content1-untracked
    reverting content1_content1_content3-tracked
    undeleting content1_content1_content3-untracked
    reverting content1_content1_missing-tracked
    undeleting content1_content1_missing-untracked
    undeleting content1_content2_content1-untracked
    reverting content1_content2_content2-tracked
    undeleting content1_content2_content2-untracked
    reverting content1_content2_content3-tracked
    undeleting content1_content2_content3-untracked
    reverting content1_content2_missing-tracked
    undeleting content1_content2_missing-untracked
    adding content1_missing_content1-untracked
    reverting content1_missing_content3-tracked
    adding content1_missing_content3-untracked
    reverting content1_missing_missing-tracked
    adding content1_missing_missing-untracked
    removing missing_content2_content2-tracked
    removing missing_content2_content3-tracked
    removing missing_content2_missing-tracked
    forgetting missing_missing_content3-tracked
    forgetting missing_missing_missing-tracked"""

# Compare resulting directory with revert target.

# The diff is filtered to include change only. The only difference should be
# additional `.orig` backup file when applicable.

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content3 content1_content1_content3-tracked.orig
    content1 content1_content1_content3-untracked
    content3 content1_content1_content3-untracked.orig
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content1 content1_content2_content1-tracked
    content1 content1_content2_content1-untracked
    content1 content1_content2_content2-tracked
    content1 content1_content2_content2-untracked
    content2 content1_content2_content2-untracked.orig
    content1 content1_content2_content3-tracked
    content3 content1_content2_content3-tracked.orig
    content1 content1_content2_content3-untracked
    content3 content1_content2_content3-untracked.orig
    content1 content1_content2_missing-tracked
    content1 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content1 content1_missing_content3-tracked
    content3 content1_missing_content3-tracked.orig
    content1 content1_missing_content3-untracked
    content3 content1_missing_content3-untracked.orig
    content1 content1_missing_missing-tracked
    content1 content1_missing_missing-untracked
    content2 missing_content2_content2-untracked
    content3 missing_content2_content3-tracked.orig
    content3 missing_content2_content3-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)
sh % "cd .."

# Test revert to parent content with explicit file name
# -----------------------------------------------------

# (setup from reference repo)

sh % "cp -R revert-ref revert-parent-explicit"
sh % "cd revert-parent-explicit"

# revert all files individually and check the output
# (output is expected to be different than in the --all case)


files = generateworkingcopystates.main("filelist", 2)
output = []
for myfile in files.split("\n"):
    output.append("### revert for: {}".format(myfile))
    output.append((sh % "hg revert {}".format(myfile)).output)

eq(
    "\n".join(output),
    r"""
    ### revert for: content1_content1_content1-tracked
    no changes needed to content1_content1_content1-tracked
    ### revert for: content1_content1_content1-untracked

    ### revert for: content1_content1_content3-tracked

    ### revert for: content1_content1_content3-untracked

    ### revert for: content1_content1_missing-tracked

    ### revert for: content1_content1_missing-untracked

    ### revert for: content1_content2_content1-tracked

    ### revert for: content1_content2_content1-untracked

    ### revert for: content1_content2_content2-tracked
    no changes needed to content1_content2_content2-tracked
    ### revert for: content1_content2_content2-untracked

    ### revert for: content1_content2_content3-tracked

    ### revert for: content1_content2_content3-untracked

    ### revert for: content1_content2_missing-tracked

    ### revert for: content1_content2_missing-untracked

    ### revert for: content1_missing_content1-tracked

    ### revert for: content1_missing_content1-untracked
    file not managed: content1_missing_content1-untracked
    ### revert for: content1_missing_content3-tracked

    ### revert for: content1_missing_content3-untracked
    file not managed: content1_missing_content3-untracked
    ### revert for: content1_missing_missing-tracked

    ### revert for: content1_missing_missing-untracked
    content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
    ### revert for: missing_content2_content2-tracked
    no changes needed to missing_content2_content2-tracked
    ### revert for: missing_content2_content2-untracked

    ### revert for: missing_content2_content3-tracked

    ### revert for: missing_content2_content3-untracked

    ### revert for: missing_content2_missing-tracked

    ### revert for: missing_content2_missing-untracked

    ### revert for: missing_missing_content3-tracked

    ### revert for: missing_missing_content3-untracked
    file not managed: missing_missing_content3-untracked
    ### revert for: missing_missing_missing-tracked

    ### revert for: missing_missing_missing-untracked
    missing_missing_missing-untracked: no such file in rev cbcb7147d2a0""",
)

# check resulting directory against the --all run
# (There should be no difference)

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content3 content1_content1_content3-tracked.orig
    content1 content1_content1_content3-untracked
    content3 content1_content1_content3-untracked.orig
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content2 content1_content2_content1-tracked
    content1 content1_content2_content1-tracked.orig
    content2 content1_content2_content1-untracked
    content1 content1_content2_content1-untracked.orig
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content2 content1_content2_content3-tracked
    content3 content1_content2_content3-tracked.orig
    content2 content1_content2_content3-untracked
    content3 content1_content2_content3-untracked.orig
    content2 content1_content2_missing-tracked
    content2 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content3 content1_missing_content3-tracked
    content3 content1_missing_content3-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content2 missing_content2_content3-tracked
    content3 missing_content2_content3-tracked.orig
    content2 missing_content2_content3-untracked
    content3 missing_content2_content3-untracked.orig
    content2 missing_content2_missing-tracked
    content2 missing_content2_missing-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)
sh % "cd .."

# Test revert to "base" content with explicit file name
# -----------------------------------------------------

# (setup from reference repo)

sh % "cp -R revert-ref revert-base-explicit"
sh % "cd revert-base-explicit"

# revert all files individually and check the output
# (output is expected to be different than in the --all case)


files = generateworkingcopystates.main("filelist", 2)
output = []
for myfile in files.split("\n"):
    output.append("### revert for: {}".format(myfile))
    output.append((sh % "hg revert {}".format(myfile)).output)

eq(
    "\n".join(output),
    r"""
    ### revert for: content1_content1_content1-tracked
    no changes needed to content1_content1_content1-tracked
    ### revert for: content1_content1_content1-untracked

    ### revert for: content1_content1_content3-tracked

    ### revert for: content1_content1_content3-untracked

    ### revert for: content1_content1_missing-tracked

    ### revert for: content1_content1_missing-untracked

    ### revert for: content1_content2_content1-tracked

    ### revert for: content1_content2_content1-untracked

    ### revert for: content1_content2_content2-tracked
    no changes needed to content1_content2_content2-tracked
    ### revert for: content1_content2_content2-untracked

    ### revert for: content1_content2_content3-tracked

    ### revert for: content1_content2_content3-untracked

    ### revert for: content1_content2_missing-tracked

    ### revert for: content1_content2_missing-untracked

    ### revert for: content1_missing_content1-tracked

    ### revert for: content1_missing_content1-untracked
    file not managed: content1_missing_content1-untracked
    ### revert for: content1_missing_content3-tracked

    ### revert for: content1_missing_content3-untracked
    file not managed: content1_missing_content3-untracked
    ### revert for: content1_missing_missing-tracked

    ### revert for: content1_missing_missing-untracked
    content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
    ### revert for: missing_content2_content2-tracked
    no changes needed to missing_content2_content2-tracked
    ### revert for: missing_content2_content2-untracked

    ### revert for: missing_content2_content3-tracked

    ### revert for: missing_content2_content3-untracked

    ### revert for: missing_content2_missing-tracked

    ### revert for: missing_content2_missing-untracked

    ### revert for: missing_missing_content3-tracked

    ### revert for: missing_missing_content3-untracked
    file not managed: missing_missing_content3-untracked
    ### revert for: missing_missing_missing-tracked

    ### revert for: missing_missing_missing-untracked
    missing_missing_missing-untracked: no such file in rev cbcb7147d2a0""",
)

# check resulting directory against the --all run
# (There should be no difference)

eq(
    dircontent(),
    r"""
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content3 content1_content1_content3-tracked.orig
    content1 content1_content1_content3-untracked
    content3 content1_content1_content3-untracked.orig
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content2 content1_content2_content1-tracked
    content1 content1_content2_content1-tracked.orig
    content2 content1_content2_content1-untracked
    content1 content1_content2_content1-untracked.orig
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content2 content1_content2_content3-tracked
    content3 content1_content2_content3-tracked.orig
    content2 content1_content2_content3-untracked
    content3 content1_content2_content3-untracked.orig
    content2 content1_content2_missing-tracked
    content2 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content3 content1_missing_content3-tracked
    content3 content1_missing_content3-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content2 missing_content2_content3-tracked
    content3 missing_content2_content3-tracked.orig
    content2 missing_content2_content3-untracked
    content3 missing_content2_content3-untracked.orig
    content2 missing_content2_missing-tracked
    content2 missing_content2_missing-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)
sh % "cd .."

# Test revert to parent content with explicit file name but ignored files
# -----------------------------------------------------------------------

# (setup from reference repo)

sh % "cp -R revert-ref revert-parent-explicit-ignored"
sh % "cd revert-parent-explicit-ignored"
sh % "echo *" > ".gitignore"

# revert all files individually and check the output
# (output is expected to be different than in the --all case)

files = generateworkingcopystates.main("filelist", 2)
output = []
for myfile in files.split("\n"):
    output.append("### revert for: {}".format(myfile))
    output.append((sh % "hg revert {}".format(myfile)).output)

eq(
    "\n".join(output),
    r"""
    ### revert for: content1_content1_content1-tracked
    no changes needed to content1_content1_content1-tracked
    ### revert for: content1_content1_content1-untracked

    ### revert for: content1_content1_content3-tracked

    ### revert for: content1_content1_content3-untracked

    ### revert for: content1_content1_missing-tracked

    ### revert for: content1_content1_missing-untracked

    ### revert for: content1_content2_content1-tracked

    ### revert for: content1_content2_content1-untracked

    ### revert for: content1_content2_content2-tracked
    no changes needed to content1_content2_content2-tracked
    ### revert for: content1_content2_content2-untracked

    ### revert for: content1_content2_content3-tracked

    ### revert for: content1_content2_content3-untracked

    ### revert for: content1_content2_missing-tracked

    ### revert for: content1_content2_missing-untracked

    ### revert for: content1_missing_content1-tracked

    ### revert for: content1_missing_content1-untracked
    file not managed: content1_missing_content1-untracked
    ### revert for: content1_missing_content3-tracked

    ### revert for: content1_missing_content3-untracked
    file not managed: content1_missing_content3-untracked
    ### revert for: content1_missing_missing-tracked

    ### revert for: content1_missing_missing-untracked
    content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
    ### revert for: missing_content2_content2-tracked
    no changes needed to missing_content2_content2-tracked
    ### revert for: missing_content2_content2-untracked

    ### revert for: missing_content2_content3-tracked

    ### revert for: missing_content2_content3-untracked

    ### revert for: missing_content2_missing-tracked

    ### revert for: missing_content2_missing-untracked

    ### revert for: missing_missing_content3-tracked

    ### revert for: missing_missing_content3-untracked
    file not managed: missing_missing_content3-untracked
    ### revert for: missing_missing_missing-tracked

    ### revert for: missing_missing_missing-untracked
    missing_missing_missing-untracked: no such file in rev cbcb7147d2a0""",
)

# check resulting directory against the --all run
# (There should be no difference)

eq(
    dircontent(),
    r"""
    content1_content1_content1-tracked content1_content1_content1-untracked content1_content1_content3-tracked content1_content1_content3-untracked content1_content2_content1-tracked content1_content2_content1-untracked content1_content2_content2-tracked content1_content2_content2-untracked content1_content2_content3-tracked content1_content2_content3-untracked content1_missing_content1-tracked content1_missing_content1-untracked content1_missing_content3-tracked content1_missing_content3-untracked missing_content2_content2-tracked missing_content2_content2-untracked missing_content2_content3-tracked missing_content2_content3-untracked missing_missing_content3-tracked missing_missing_content3-untracked .gitignore
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content3 content1_content1_content3-tracked.orig
    content1 content1_content1_content3-untracked
    content3 content1_content1_content3-untracked.orig
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content2 content1_content2_content1-tracked
    content1 content1_content2_content1-tracked.orig
    content2 content1_content2_content1-untracked
    content1 content1_content2_content1-untracked.orig
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content2 content1_content2_content3-tracked
    content3 content1_content2_content3-tracked.orig
    content2 content1_content2_content3-untracked
    content3 content1_content2_content3-untracked.orig
    content2 content1_content2_missing-tracked
    content2 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content3 content1_missing_content3-tracked
    content3 content1_missing_content3-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content2 missing_content2_content3-tracked
    content3 missing_content2_content3-tracked.orig
    content2 missing_content2_content3-untracked
    content3 missing_content2_content3-untracked.orig
    content2 missing_content2_missing-tracked
    content2 missing_content2_missing-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)
sh % "cd .."

# Test revert to "base" content with explicit file name
# -----------------------------------------------------

# (setup from reference repo)

sh % "cp -R revert-ref revert-base-explicit-ignored"
sh % "cd revert-base-explicit-ignored"
sh % "echo *" > ".gitignore"

# revert all files individually and check the output
# (output is expected to be different than in the --all case)


files = generateworkingcopystates.main("filelist", 2)
output = []
for myfile in files.split("\n"):
    output.append("### revert for: {}".format(myfile))
    output.append((sh % "hg revert {}".format(myfile)).output)
eq(
    "\n".join(output),
    r"""
    ### revert for: content1_content1_content1-tracked
    no changes needed to content1_content1_content1-tracked
    ### revert for: content1_content1_content1-untracked

    ### revert for: content1_content1_content3-tracked

    ### revert for: content1_content1_content3-untracked

    ### revert for: content1_content1_missing-tracked

    ### revert for: content1_content1_missing-untracked

    ### revert for: content1_content2_content1-tracked

    ### revert for: content1_content2_content1-untracked

    ### revert for: content1_content2_content2-tracked
    no changes needed to content1_content2_content2-tracked
    ### revert for: content1_content2_content2-untracked

    ### revert for: content1_content2_content3-tracked

    ### revert for: content1_content2_content3-untracked

    ### revert for: content1_content2_missing-tracked

    ### revert for: content1_content2_missing-untracked

    ### revert for: content1_missing_content1-tracked

    ### revert for: content1_missing_content1-untracked
    file not managed: content1_missing_content1-untracked
    ### revert for: content1_missing_content3-tracked

    ### revert for: content1_missing_content3-untracked
    file not managed: content1_missing_content3-untracked
    ### revert for: content1_missing_missing-tracked

    ### revert for: content1_missing_missing-untracked
    content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
    ### revert for: missing_content2_content2-tracked
    no changes needed to missing_content2_content2-tracked
    ### revert for: missing_content2_content2-untracked

    ### revert for: missing_content2_content3-tracked

    ### revert for: missing_content2_content3-untracked

    ### revert for: missing_content2_missing-tracked

    ### revert for: missing_content2_missing-untracked

    ### revert for: missing_missing_content3-tracked

    ### revert for: missing_missing_content3-untracked
    file not managed: missing_missing_content3-untracked
    ### revert for: missing_missing_missing-tracked

    ### revert for: missing_missing_missing-untracked
    missing_missing_missing-untracked: no such file in rev cbcb7147d2a0""",
)

# check resulting directory against the --all run
# (There should be no difference)

eq(
    dircontent(),
    r"""
    content1_content1_content1-tracked content1_content1_content1-untracked content1_content1_content3-tracked content1_content1_content3-untracked content1_content2_content1-tracked content1_content2_content1-untracked content1_content2_content2-tracked content1_content2_content2-untracked content1_content2_content3-tracked content1_content2_content3-untracked content1_missing_content1-tracked content1_missing_content1-untracked content1_missing_content3-tracked content1_missing_content3-untracked missing_content2_content2-tracked missing_content2_content2-untracked missing_content2_content3-tracked missing_content2_content3-untracked missing_missing_content3-tracked missing_missing_content3-untracked .gitignore
    content1 content1_content1_content1-tracked
    content1 content1_content1_content1-untracked
    content1 content1_content1_content3-tracked
    content3 content1_content1_content3-tracked.orig
    content1 content1_content1_content3-untracked
    content3 content1_content1_content3-untracked.orig
    content1 content1_content1_missing-tracked
    content1 content1_content1_missing-untracked
    content2 content1_content2_content1-tracked
    content1 content1_content2_content1-tracked.orig
    content2 content1_content2_content1-untracked
    content1 content1_content2_content1-untracked.orig
    content2 content1_content2_content2-tracked
    content2 content1_content2_content2-untracked
    content2 content1_content2_content3-tracked
    content3 content1_content2_content3-tracked.orig
    content2 content1_content2_content3-untracked
    content3 content1_content2_content3-untracked.orig
    content2 content1_content2_missing-tracked
    content2 content1_content2_missing-untracked
    content1 content1_missing_content1-tracked
    content1 content1_missing_content1-untracked
    content3 content1_missing_content3-tracked
    content3 content1_missing_content3-untracked
    content2 missing_content2_content2-tracked
    content2 missing_content2_content2-untracked
    content2 missing_content2_content3-tracked
    content3 missing_content2_content3-tracked.orig
    content2 missing_content2_content3-untracked
    content3 missing_content2_content3-untracked.orig
    content2 missing_content2_missing-tracked
    content2 missing_content2_missing-untracked
    content3 missing_missing_content3-tracked
    content3 missing_missing_content3-untracked""",
)
sh % "cd .."

# Revert to an ancestor of P2 during a merge (issue5052)
# -----------------------------------------------------

# (prepare the repository)

sh % "hg init issue5052"
sh % "cd issue5052"
sh % "echo '*\\.orig'" > ".gitignore"
sh % "echo 0" > "root"
sh % "hg ci -qAm C0"
sh % "echo 0" > "A"
sh % "hg ci -qAm C1"
sh % "echo 1" >> "A"
sh % "hg ci -qm C2"
sh % "hg up -q 0"
sh % "echo 1" > "B"
sh % "hg ci -qAm C3"
sh % "hg status --rev 'ancestor(.,2)' --rev 2" == "A A"
sh % "hg log -G -T '{rev} ({files})\\n'" == r"""
    @  3 (B)
    |
    | o  2 (A)
    | |
    | o  1 (A)
    |/
    o  0 (.gitignore root)"""

# actual tests: reverting to something else than a merge parent

sh % "hg merge" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

sh % "hg status --rev 'p1()'" == "M A"
sh % "hg status --rev 'p2()'" == "A B"
sh % "hg status --rev 1" == r"""
    M A
    A B"""
sh % "hg revert --rev 1 --all" == r"""
    reverting A
    removing B"""
sh % "hg status --rev 1"

# From the other parents

sh % "hg up -C 'p2()'" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg merge" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

sh % "hg status --rev 'p1()'" == "M B"
sh % "hg status --rev 'p2()'" == "A A"
sh % "hg status --rev 1" == r"""
    M A
    A B"""
sh % "hg revert --rev 1 --all" == r"""
    reverting A
    removing B"""
sh % "hg status --rev 1"

sh % "cd .."
