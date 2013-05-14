  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
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
  # Commands:
  #  p, pick = use commit
  #  e, edit = use commit, but stop for amending
  #  f, fold = use commit, but fold into previous commit (combines N and N-1)
  #  d, drop = remove commit from history
  #  m, mess = edit message without changing commit content
  #
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

edit the history
(use a hacky editor to check histedit-last-edit.txt backup)

  $ EDITED="$TESTTMP/editedhistory"
  $ cat > $EDITED <<EOF
  > pick 177f92b77385 c
  > pick e860deea161a e
  > pick 652413bf663e f
  > pick 055a42cdd887 d
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

rules should end up in .hg/histedit-last-edit.txt:
  $ cat .hg/histedit-last-edit.txt
  pick 177f92b77385 c
  pick e860deea161a e
  pick 652413bf663e f
  pick 055a42cdd887 d

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
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  abort: may not use changesets other than the ones listed
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
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..
