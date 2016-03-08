  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > EOF

  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  > }

  $ initrepo

log before edit
  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

show the edit commands offered
  $ HGEDITOR=cat hg histedit 177f92b77385
  pick 177f92b77385 2 c
  pick 055a42cdd887 3 d
  pick e860deea161a 4 e
  pick 652413bf663e 5 f
  
  # Edit history between 177f92b77385 and 652413bf663e
  #
  # Commits are listed from least to most recent
  #
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #

edit the history
(use a hacky editor to check histedit-last-edit.txt backup)

  $ EDITED="$TESTTMP/editedhistory"
  $ cat > $EDITED <<EOF
  > edit 177f92b77385 c
  > pick e860deea161a e
  > pick 652413bf663e f
  > pick 055a42cdd887 d
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  Editing (177f92b77385), you may commit or record as needed now.
  (hg histedit --continue to resume)

rules should end up in .hg/histedit-last-edit.txt:
  $ cat .hg/histedit-last-edit.txt
  edit 177f92b77385 c
  pick e860deea161a e
  pick 652413bf663e f
  pick 055a42cdd887 d

  $ hg histedit --abort
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat > $EDITED <<EOF
  > pick 177f92b77385 c
  > pick e860deea161a e
  > pick 652413bf663e f
  > pick 055a42cdd887 d
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle

log after edit
  $ hg log --graph
  @  changeset:   5:07114f51870f
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   4:8ade9693061e
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   3:d8249471110a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

put things back

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF | fixbundle
  > pick 177f92b77385 c
  > pick 07114f51870f d
  > pick d8249471110a e
  > pick 8ade9693061e f
  > EOF

  $ hg log --graph
  @  changeset:   5:7eca9b5b1148
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:915da888f2de
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:10517e47bbbb
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

slightly different this time

  $ hg histedit 177f92b77385 --commands - << EOF 2>&1 | fixbundle
  > pick 10517e47bbbb d
  > pick 7eca9b5b1148 f
  > pick 915da888f2de e
  > pick 177f92b77385 c
  > EOF
  $ hg log --graph
  @  changeset:   5:38b92f448761
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   4:de71b079d9ce
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:be9ae3a309c6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:799205341b6b
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

keep prevents stripping dead revs
  $ hg histedit 799205341b6b --keep --commands - 2>&1 << EOF | fixbundle
  > pick 799205341b6b d
  > pick be9ae3a309c6 f
  > pick 38b92f448761 c
  > pick de71b079d9ce e
  > EOF
  $ hg log --graph
  @  changeset:   7:803ef1c6fcfd
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   6:ece0b8d93dda
  |  parent:      3:be9ae3a309c6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  | o  changeset:   5:38b92f448761
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     c
  | |
  | o  changeset:   4:de71b079d9ce
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     e
  |
  o  changeset:   3:be9ae3a309c6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:799205341b6b
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

try with --rev
  $ hg histedit --commands - --rev -2 2>&1 <<EOF | fixbundle
  > pick de71b079d9ce e
  > pick 38b92f448761 c
  > EOF
  hg: parse error: pick "de71b079d9ce" changeset was not a candidate
  (only use listed changesets)
  $ hg log --graph
  @  changeset:   7:803ef1c6fcfd
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   6:ece0b8d93dda
  |  parent:      3:be9ae3a309c6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  | o  changeset:   5:38b92f448761
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     c
  | |
  | o  changeset:   4:de71b079d9ce
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     e
  |
  o  changeset:   3:be9ae3a309c6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:799205341b6b
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
Verify that revsetalias entries work with histedit:
  $ cat >> $HGRCPATH <<EOF
  > [revsetalias]
  > grandparent(ARG) = p1(p1(ARG))
  > EOF
  $ echo extra commit >> c
  $ hg ci -m 'extra commit to c'
  $ HGEDITOR=cat hg histedit 'grandparent(.)'
  pick ece0b8d93dda 6 c
  pick 803ef1c6fcfd 7 e
  pick 9c863c565126 8 extra commit to c
  
  # Edit history between ece0b8d93dda and 9c863c565126
  #
  # Commits are listed from least to most recent
  #
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #

should also work if a commit message is missing
  $ BUNDLE="$TESTDIR/missing-comment.hg"
  $ hg init missing
  $ cd missing
  $ hg unbundle $BUNDLE
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg co tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log --graph
  @  changeset:   2:bd22688093b3
  |  tag:         tip
  |  user:        Robert Altman <robert.altman@telventDTN.com>
  |  date:        Mon Nov 28 16:40:04 2011 +0000
  |  summary:     Update file.
  |
  o  changeset:   1:3b3e956f9171
  |  user:        Robert Altman <robert.altman@telventDTN.com>
  |  date:        Mon Nov 28 16:37:57 2011 +0000
  |
  o  changeset:   0:141947992243
     user:        Robert Altman <robert.altman@telventDTN.com>
     date:        Mon Nov 28 16:35:28 2011 +0000
     summary:     Checked in text file
  
  $ hg histedit 0
  $ cd ..

  $ cd ..


Test to make sure folding renames doesn't cause bogus conflicts (issue4251):
  $ hg init issue4251
  $ cd issue4251

  $ mkdir initial-dir
  $ echo foo > initial-dir/initial-file
  $ hg add initial-dir/initial-file
  $ hg commit -m "initial commit"

Move the file to a new directory, and in the same commit, change its content:
  $ mkdir another-dir
  $ hg mv initial-dir/initial-file another-dir/
  $ echo changed > another-dir/initial-file
  $ hg commit -m "moved and changed"

Rename the file:
  $ hg mv another-dir/initial-file another-dir/renamed-file
  $ hg commit -m "renamed"

Now, let's try to fold the second commit into the first:
  $ cat > editor.sh <<EOF
  > #!/bin/sh
  > cat > \$1 <<ENDOF
  > pick b0f4233702ca 0 initial commit
  > fold 5e8704a8f2d2 1 moved and changed
  > pick 40e7299e8fa7 2 renamed
  > ENDOF
  > EOF

  $ HGEDITOR="sh ./editor.sh" hg histedit 0
  saved backup bundle to $TESTTMP/issue4251/.hg/strip-backup/*-backup.hg (glob)
  saved backup bundle to $TESTTMP/issue4251/.hg/strip-backup/*-backup.hg (glob)

  $ hg --config diff.git=yes export 0
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID fffadc26f8f85623ce60b028a3f1ccc3730f8530
  # Parent  0000000000000000000000000000000000000000
  pick b0f4233702ca 0 initial commit
  fold 5e8704a8f2d2 1 moved and changed
  pick 40e7299e8fa7 2 renamed
  
  diff --git a/another-dir/initial-file b/another-dir/initial-file
  new file mode 100644
  --- /dev/null
  +++ b/another-dir/initial-file
  @@ -0,0 +1,1 @@
  +changed

  $ hg --config diff.git=yes export 1
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 9b730d82b00af8a2766facebfa47cc124405a118
  # Parent  fffadc26f8f85623ce60b028a3f1ccc3730f8530
  renamed
  
  diff --git a/another-dir/initial-file b/another-dir/renamed-file
  rename from another-dir/initial-file
  rename to another-dir/renamed-file

  $ cd ..
