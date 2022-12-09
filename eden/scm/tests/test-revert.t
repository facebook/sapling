#debugruntest-compatible
# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

# Prepare test functions

  import os
  import generateworkingcopystates
  
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

# init 

  $ hg init repo
  $ cd repo
  $ echo 123 > a
  $ echo 123 > c
  $ echo 123 > e
  $ hg add a c e
  $ hg commit -m first a c e

# nothing changed

  $ hg revert
  abort: no files or directories specified
  (use --all to revert all files)
  [255]
  $ hg revert --all

# Introduce some changes and revert them
# --------------------------------------

  $ echo 123 > b

  $ hg status
  ? b
  $ echo 12 > c

  $ hg status
  M c
  ? b
  $ hg add b

  $ hg status
  M c
  A b
  $ hg rm a

  $ hg status
  M c
  A b
  R a

# revert removal of a file

  $ hg revert a
  $ hg status
  M c
  A b

# revert addition of a file

  $ hg revert b
  $ hg status
  M c
  ? b

# revert modification of a file (--no-backup)

  $ hg revert --no-backup c
  $ hg status
  ? b

# revert deletion (! status) of a added file
# ------------------------------------------

  $ hg add b

  $ hg status b
  A b
  $ rm b
  $ hg status b
  ! b
  $ hg revert -v b
  forgetting b
  $ hg status b
  b: * (glob)

  $ ls
  a
  c
  e

# Test creation of backup (.orig) files
# -------------------------------------

  $ echo z > e
  $ hg revert --all -v
  saving current version of e as e.orig
  reverting e

# Test creation of backup (.orig) file in configured file location
# ----------------------------------------------------------------

  $ echo z > e
  $ hg revert --all -v --config 'ui.origbackuppath=.hg/origbackups'
  creating directory: $TESTTMP/repo/.hg/origbackups
  saving current version of e as $TESTTMP/repo/.hg/origbackups/e
  reverting e
  $ rm -rf .hg/origbackups

# revert on clean file (no change)
# --------------------------------

  $ hg revert a
  no changes needed to a

  $ hg revert -q a

# revert on an untracked file
# ---------------------------

  $ echo q > q
  $ hg revert q
  file not managed: q
  $ rm q

# revert on file that does not exists
# -----------------------------------

  $ hg revert notfound
  notfound: no such file in rev 334a9e57682c
  $ touch d
  $ hg add d
  $ hg rm a
  $ hg commit -m second
  $ echo z > z
  $ hg add z
  $ hg st
  A z
  ? e.orig

# revert to another revision (--rev)
# ----------------------------------

  $ hg revert --all -r0
  adding a
  removing d
  forgetting z

# revert explicitly to parent (--rev)
# -----------------------------------

  $ hg revert --all -rtip
  forgetting a
  undeleting d
  $ rm a *.orig

# revert to another revision (--rev) and exact match
# --------------------------------------------------
# exact match are more silent

  $ hg revert -r0 a
  $ hg st a
  A a
  $ hg rm d
  $ hg st d
  R d

# should keep d removed

  $ hg revert -r0 d
  no changes needed to d
  $ hg st d
  R d

  $ hg goto -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

#if execbit
  $ chmod +x c
  $ hg revert --all
  reverting c

  $ test -x c
  [1]

  $ chmod +x c
  $ hg commit -m exe

  $ chmod -x c
  $ hg revert --all
  reverting c

  $ test -x c
  $ echo executable
  executable
#endif

# Test that files reverted to other than the parent are treated as
# "modified", even if none of mode, size and timestamp of it isn't
# changed on the filesystem (see also issue4583).

  $ echo 321 > e
  $ hg diff --git
  diff --git a/e b/e
  --- a/e
  +++ b/e
  @@ -1,1 +1,1 @@
  -123
  +321
  $ hg commit -m 'ambiguity from size'

  $ cat e
  321
  $ touch -t 200001010000 e
  $ hg debugrebuildstate

  $ cat >> .hg/hgrc << 'EOF'
  > [fakedirstatewritetime]
  > # emulate invoking dirstate.write() via repo.status()
  > # at 2000-01-01 00:00
  > fakenow = 2000-01-01 00:00:00
  > 
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > EOF
  $ hg revert -r 0 e
  $ cat >> .hg/hgrc << 'EOF'
  > [extensions]
  > fakedirstatewritetime = !
  > EOF

  $ cat e
  123
  $ touch -t 200001010000 e
  $ hg status -A e
  M e

  $ cd ..

# Issue241: update and revert produces inconsistent repositories
# --------------------------------------------------------------

  $ hg init a
  $ cd a
  $ echo a >> a
  $ hg commit -A -d '1 0' -m a
  adding a
  $ echo a >> a
  $ hg commit -d '2 0' -m a
  $ hg goto 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir b
  $ echo b > b/b

# call `hg revert` with no file specified
# ---------------------------------------

  $ hg revert -rtip
  abort: no files or directories specified
  (use --all to revert all files, or 'hg goto 1' to update)
  [255]

# call `hg revert` with -I
# ---------------------------

  $ echo a >> a
  $ hg revert -I a
  reverting a

# call `hg revert` with -X
# ---------------------------

  $ echo a >> a
  $ hg revert -X d
  reverting a

# call `hg revert` with --all
# ---------------------------

  $ hg revert --all -rtip
  reverting a
  $ rm 'a.orig'

# Issue332: confusing message when reverting directory
# ----------------------------------------------------

  $ hg ci -A -m b
  adding b/b
  $ echo foobar > b/b
  $ mkdir newdir
  $ echo foo > newdir/newfile
  $ hg add newdir/newfile
  $ hg revert b newdir
  reverting b/b
  forgetting newdir/newfile
  $ echo foobar > b/b
  $ hg revert .
  reverting b/b

# reverting a rename target should revert the source
# --------------------------------------------------

  $ hg mv a newa
  $ hg revert newa
  $ hg st a newa
  ? newa

# Also true for move overwriting an existing file

  $ hg mv --force a b/b
  $ hg revert b/b
  $ hg status a b/b

  $ cd ..

  $ hg init ignored
  $ cd ignored
  $ echo ignored > .gitignore
  $ echo ignoreddir >> .gitignore
  $ echo removed >> .gitignore

  $ mkdir ignoreddir
  $ touch ignoreddir/file
  $ touch ignoreddir/removed
  $ touch ignored
  $ touch removed

# 4 ignored files (we will add/commit everything)

  $ hg st -A -X .gitignore
  I ignored
  I ignoreddir/file
  I ignoreddir/removed
  I removed
  $ hg ci -qAm 'add files' ignored ignoreddir/file ignoreddir/removed removed

  $ echo >> ignored
  $ echo >> ignoreddir/file
  $ hg rm removed ignoreddir/removed

# should revert ignored* and undelete *removed
# --------------------------------------------

  $ hg revert -a --no-backup
  reverting ignored
  reverting ignoreddir/file
  undeleting ignoreddir/removed
  undeleting removed
  $ hg st -mardi

  $ hg up -qC
  $ echo >> ignored
  $ hg rm removed

# should silently revert the named files
# --------------------------------------

  $ hg revert --no-backup ignored removed
  $ hg st -mardi

# Reverting copy (issue3920)
# --------------------------
# someone set up us the copies

  $ rm .gitignore
  $ hg goto -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv ignored allyour
  $ hg copy removed base
  $ hg commit -m rename

# copies and renames, you have no chance to survive make your time (issue3920)

  $ hg goto '.^'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg revert -rtip -a
  adding allyour
  adding base
  removing ignored
  $ hg status -C
  A allyour
    ignored
  A base
    removed
  R ignored

# Test revert of a file added by one side of the merge
# ====================================================
# remove any pending change

  $ hg revert --all
  forgetting allyour
  forgetting base
  undeleting ignored
  $ hg purge --all

# Adds a new commit

  $ echo foo > newadd
  $ hg add newadd
  $ hg commit -m 'other adds'

# merge it with the other head

  $ hg merge
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg summary
  parent: 68b989552c4a 
   other adds
  parent: 2b80f4f4fe72 
   rename
  commit: 2 modified, 1 removed (merge)
  phases: 3 draft

# clarifies who added what

  $ hg status
  M allyour
  M base
  R ignored
  $ hg status --change 'p1()'
  A newadd
  $ hg status --change 'p2()'
  A allyour
  A base
  R ignored

# revert file added by p1() to p1() state
# -----------------------------------------

  $ hg revert -r 'p1()' 'glob:newad?'
  $ hg status
  M allyour
  M base
  R ignored

# revert file added by p1() to p2() state
# ------------------------------------------

  $ hg revert -r 'p2()' 'glob:newad?'
  removing newadd
  $ hg status
  M allyour
  M base
  R ignored
  R newadd

# revert file added by p2() to p2() state
# ------------------------------------------

  $ hg revert -r 'p2()' 'glob:allyou?'
  $ hg status
  M allyour
  M base
  R ignored
  R newadd

# revert file added by p2() to p1() state
# ------------------------------------------

  $ hg revert -r 'p1()' 'glob:allyou?'
  removing allyour
  $ hg status
  M base
  R allyour
  R ignored
  R newadd

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

  >>> print(generateworkingcopystates.main("filelist", 2))
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
  missing_missing_missing-untracked


# Script to make a simple text version of the content
# ---------------------------------------------------
# Generate appropriate repo state
# -------------------------------

  $ hg init revert-ref
  $ cd revert-ref

# Generate base changeset

  >>> generateworkingcopystates.main("state", 2, 1)

  $ hg addremove --similarity 0
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
  adding content1_missing_missing-untracked
  $ hg status
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
  A content1_missing_missing-untracked
  $ hg commit -m base

# (create a simple text version of the content)

  >>> print(dircontent())
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
  content1 content1_missing_missing-untracked


# Create parent changeset

  >>> generateworkingcopystates.main("state", 2, 2)

  $ hg addremove --similarity 0
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
  adding missing_content2_missing-untracked
  $ hg status
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
  R content1_missing_missing-untracked
  $ hg commit -m parent

# (create a simple text version of the content)

  >>> print(dircontent())
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
  content2 missing_content2_missing-untracked


# Setup working directory

  >>> generateworkingcopystates.main("state", 2, "wc")

  $ hg addremove --similarity 0
  adding content1_missing_content1-tracked
  adding content1_missing_content1-untracked
  adding content1_missing_content3-tracked
  adding content1_missing_content3-untracked
  adding content1_missing_missing-tracked
  adding content1_missing_missing-untracked
  adding missing_missing_content3-tracked
  adding missing_missing_content3-untracked
  adding missing_missing_missing-tracked
  adding missing_missing_missing-untracked
  $ hg forget *_*_*-untracked
  $ rm *_*_missing-*
  $ hg status
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
  ? missing_missing_content3-untracked

  $ hg status --rev 'desc("base")'
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
  ? missing_missing_content3-untracked

# (create a simple text version of the content)

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Test revert --all to parent content
# -----------------------------------
# (setup from reference repo)

  $ cp -R revert-ref revert-parent-all
  $ cd revert-parent-all

# check revert output

  $ hg revert --all
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
  forgetting missing_missing_missing-tracked

# Compare resulting directory with revert target.

# The diff is filtered to include change only. The only difference should be
# additional `.orig` backup file when applicable.

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Test revert --all to "base" content
# -----------------------------------
# (setup from reference repo)

  $ cp -R revert-ref revert-base-all
  $ cd revert-base-all

# check revert output

  $ hg revert --all --rev 'desc(base)'
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
  forgetting missing_missing_missing-tracked

# Compare resulting directory with revert target.

# The diff is filtered to include change only. The only difference should be
# additional `.orig` backup file when applicable.

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Test revert to parent content with explicit file name
# -----------------------------------------------------
# (setup from reference repo)

  $ cp -R revert-ref revert-parent-explicit
  $ cd revert-parent-explicit

# revert all files individually and check the output
# (output is expected to be different than in the --all case)

  >>> def revertoutput():
  ...     files = generateworkingcopystates.main("filelist", 2)
  ...     output = []
  ...     for myfile in files.split("\n"):
  ...         output.append("### revert for: {}".format(myfile))
  ...         output.append(sheval("hg revert {}".format(myfile)).rstrip() or ".")
  ...     print("\n".join(output))

  >>> revertoutput()
  ### revert for: content1_content1_content1-tracked
  no changes needed to content1_content1_content1-tracked
  ### revert for: content1_content1_content1-untracked
  .
  ### revert for: content1_content1_content3-tracked
  .
  ### revert for: content1_content1_content3-untracked
  .
  ### revert for: content1_content1_missing-tracked
  .
  ### revert for: content1_content1_missing-untracked
  .
  ### revert for: content1_content2_content1-tracked
  .
  ### revert for: content1_content2_content1-untracked
  .
  ### revert for: content1_content2_content2-tracked
  no changes needed to content1_content2_content2-tracked
  ### revert for: content1_content2_content2-untracked
  .
  ### revert for: content1_content2_content3-tracked
  .
  ### revert for: content1_content2_content3-untracked
  .
  ### revert for: content1_content2_missing-tracked
  .
  ### revert for: content1_content2_missing-untracked
  .
  ### revert for: content1_missing_content1-tracked
  .
  ### revert for: content1_missing_content1-untracked
  file not managed: content1_missing_content1-untracked
  ### revert for: content1_missing_content3-tracked
  .
  ### revert for: content1_missing_content3-untracked
  file not managed: content1_missing_content3-untracked
  ### revert for: content1_missing_missing-tracked
  .
  ### revert for: content1_missing_missing-untracked
  content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
  ### revert for: missing_content2_content2-tracked
  no changes needed to missing_content2_content2-tracked
  ### revert for: missing_content2_content2-untracked
  .
  ### revert for: missing_content2_content3-tracked
  .
  ### revert for: missing_content2_content3-untracked
  .
  ### revert for: missing_content2_missing-tracked
  .
  ### revert for: missing_content2_missing-untracked
  .
  ### revert for: missing_missing_content3-tracked
  .
  ### revert for: missing_missing_content3-untracked
  file not managed: missing_missing_content3-untracked
  ### revert for: missing_missing_missing-tracked
  .
  ### revert for: missing_missing_missing-untracked
  missing_missing_missing-untracked: no such file in rev cbcb7147d2a0

# check resulting directory against the --all run
# (There should be no difference)

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Test revert to "base" content with explicit file name
# -----------------------------------------------------
# (setup from reference repo)

  $ cp -R revert-ref revert-base-explicit
  $ cd revert-base-explicit

# revert all files individually and check the output
# (output is expected to be different than in the --all case)


  >>> revertoutput()
  ### revert for: content1_content1_content1-tracked
  no changes needed to content1_content1_content1-tracked
  ### revert for: content1_content1_content1-untracked
  .
  ### revert for: content1_content1_content3-tracked
  .
  ### revert for: content1_content1_content3-untracked
  .
  ### revert for: content1_content1_missing-tracked
  .
  ### revert for: content1_content1_missing-untracked
  .
  ### revert for: content1_content2_content1-tracked
  .
  ### revert for: content1_content2_content1-untracked
  .
  ### revert for: content1_content2_content2-tracked
  no changes needed to content1_content2_content2-tracked
  ### revert for: content1_content2_content2-untracked
  .
  ### revert for: content1_content2_content3-tracked
  .
  ### revert for: content1_content2_content3-untracked
  .
  ### revert for: content1_content2_missing-tracked
  .
  ### revert for: content1_content2_missing-untracked
  .
  ### revert for: content1_missing_content1-tracked
  .
  ### revert for: content1_missing_content1-untracked
  file not managed: content1_missing_content1-untracked
  ### revert for: content1_missing_content3-tracked
  .
  ### revert for: content1_missing_content3-untracked
  file not managed: content1_missing_content3-untracked
  ### revert for: content1_missing_missing-tracked
  .
  ### revert for: content1_missing_missing-untracked
  content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
  ### revert for: missing_content2_content2-tracked
  no changes needed to missing_content2_content2-tracked
  ### revert for: missing_content2_content2-untracked
  .
  ### revert for: missing_content2_content3-tracked
  .
  ### revert for: missing_content2_content3-untracked
  .
  ### revert for: missing_content2_missing-tracked
  .
  ### revert for: missing_content2_missing-untracked
  .
  ### revert for: missing_missing_content3-tracked
  .
  ### revert for: missing_missing_content3-untracked
  file not managed: missing_missing_content3-untracked
  ### revert for: missing_missing_missing-tracked
  .
  ### revert for: missing_missing_missing-untracked
  missing_missing_missing-untracked: no such file in rev cbcb7147d2a0


# check resulting directory against the --all run
# (There should be no difference)

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Test revert to parent content with explicit file name but ignored files
# -----------------------------------------------------------------------
# (setup from reference repo)

  $ cp -R revert-ref revert-parent-explicit-ignored
  $ cd revert-parent-explicit-ignored
  $ echo * > .gitignore

# revert all files individually and check the output
# (output is expected to be different than in the --all case)

  >>> revertoutput()
  ### revert for: content1_content1_content1-tracked
  no changes needed to content1_content1_content1-tracked
  ### revert for: content1_content1_content1-untracked
  .
  ### revert for: content1_content1_content3-tracked
  .
  ### revert for: content1_content1_content3-untracked
  .
  ### revert for: content1_content1_missing-tracked
  .
  ### revert for: content1_content1_missing-untracked
  .
  ### revert for: content1_content2_content1-tracked
  .
  ### revert for: content1_content2_content1-untracked
  .
  ### revert for: content1_content2_content2-tracked
  no changes needed to content1_content2_content2-tracked
  ### revert for: content1_content2_content2-untracked
  .
  ### revert for: content1_content2_content3-tracked
  .
  ### revert for: content1_content2_content3-untracked
  .
  ### revert for: content1_content2_missing-tracked
  .
  ### revert for: content1_content2_missing-untracked
  .
  ### revert for: content1_missing_content1-tracked
  .
  ### revert for: content1_missing_content1-untracked
  file not managed: content1_missing_content1-untracked
  ### revert for: content1_missing_content3-tracked
  .
  ### revert for: content1_missing_content3-untracked
  file not managed: content1_missing_content3-untracked
  ### revert for: content1_missing_missing-tracked
  .
  ### revert for: content1_missing_missing-untracked
  content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
  ### revert for: missing_content2_content2-tracked
  no changes needed to missing_content2_content2-tracked
  ### revert for: missing_content2_content2-untracked
  .
  ### revert for: missing_content2_content3-tracked
  .
  ### revert for: missing_content2_content3-untracked
  .
  ### revert for: missing_content2_missing-tracked
  .
  ### revert for: missing_content2_missing-untracked
  .
  ### revert for: missing_missing_content3-tracked
  .
  ### revert for: missing_missing_content3-untracked
  file not managed: missing_missing_content3-untracked
  ### revert for: missing_missing_missing-tracked
  .
  ### revert for: missing_missing_missing-untracked
  missing_missing_missing-untracked: no such file in rev cbcb7147d2a0


# check resulting directory against the --all run

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Test revert to "base" content with explicit file name
# -----------------------------------------------------
# (setup from reference repo)

  $ cp -R revert-ref revert-base-explicit-ignored
  $ cd revert-base-explicit-ignored
  $ echo * > .gitignore

# revert all files individually and check the output
# (output is expected to be different than in the --all case)

  >>> revertoutput()
  ### revert for: content1_content1_content1-tracked
  no changes needed to content1_content1_content1-tracked
  ### revert for: content1_content1_content1-untracked
  .
  ### revert for: content1_content1_content3-tracked
  .
  ### revert for: content1_content1_content3-untracked
  .
  ### revert for: content1_content1_missing-tracked
  .
  ### revert for: content1_content1_missing-untracked
  .
  ### revert for: content1_content2_content1-tracked
  .
  ### revert for: content1_content2_content1-untracked
  .
  ### revert for: content1_content2_content2-tracked
  no changes needed to content1_content2_content2-tracked
  ### revert for: content1_content2_content2-untracked
  .
  ### revert for: content1_content2_content3-tracked
  .
  ### revert for: content1_content2_content3-untracked
  .
  ### revert for: content1_content2_missing-tracked
  .
  ### revert for: content1_content2_missing-untracked
  .
  ### revert for: content1_missing_content1-tracked
  .
  ### revert for: content1_missing_content1-untracked
  file not managed: content1_missing_content1-untracked
  ### revert for: content1_missing_content3-tracked
  .
  ### revert for: content1_missing_content3-untracked
  file not managed: content1_missing_content3-untracked
  ### revert for: content1_missing_missing-tracked
  .
  ### revert for: content1_missing_missing-untracked
  content1_missing_missing-untracked: no such file in rev cbcb7147d2a0
  ### revert for: missing_content2_content2-tracked
  no changes needed to missing_content2_content2-tracked
  ### revert for: missing_content2_content2-untracked
  .
  ### revert for: missing_content2_content3-tracked
  .
  ### revert for: missing_content2_content3-untracked
  .
  ### revert for: missing_content2_missing-tracked
  .
  ### revert for: missing_content2_missing-untracked
  .
  ### revert for: missing_missing_content3-tracked
  .
  ### revert for: missing_missing_content3-untracked
  file not managed: missing_missing_content3-untracked
  ### revert for: missing_missing_missing-tracked
  .
  ### revert for: missing_missing_missing-untracked
  missing_missing_missing-untracked: no such file in rev cbcb7147d2a0


# check resulting directory against the --all run
# (There should be no difference)

  >>> print(dircontent())
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
  content3 missing_missing_content3-untracked

  $ cd ..

# Revert to an ancestor of P2 during a merge (issue5052)
# -----------------------------------------------------
# (prepare the repository)

  $ hg init issue5052
  $ cd issue5052
  $ echo '*\.orig' > .gitignore
  $ echo 0 > root
  $ hg ci -qAm C0
  $ echo 0 > A
  $ hg ci -qAm C1
  $ echo 1 >> A
  $ hg ci -qm C2
  $ hg up -q 0
  $ echo 1 > B
  $ hg ci -qAm C3
  $ hg status --rev 'ancestor(.,2)' --rev 2
  A A
  $ hg log -G -T '{rev} ({files})\n'
  @  3 (B)
  │
  │ o  2 (A)
  │ │
  │ o  1 (A)
  ├─╯
  o  0 (.gitignore root)

# actual tests: reverting to something else than a merge parent

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg status --rev 'p1()'
  M A
  $ hg status --rev 'p2()'
  A B
  $ hg status --rev 1
  M A
  A B
  $ hg revert --rev 1 --all
  reverting A
  removing B
  $ hg status --rev 1

# From the other parents

  $ hg up -C 'p2()'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg status --rev 'p1()'
  M B
  $ hg status --rev 'p2()'
  A A
  $ hg status --rev 1
  M A
  A B
  $ hg revert --rev 1 --all
  reverting A
  removing B
  $ hg status --rev 1

# Don't backup symlink reverts

  $ ln -s foo bar
  $ hg add bar
  $ hg commit -m symlink
  $ rm bar
  $ ln -s car bar
  $ hg status
  M bar
  $ hg revert --all --config 'ui.origbackuppath=.hg/origbackups'
  reverting bar
  $ ls .hg/origbackups
