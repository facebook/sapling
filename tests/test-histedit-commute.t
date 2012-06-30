  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > histedit=
  > EOF

  $ EDITED=`pwd`/editedhistory
  $ cat > $EDITED <<EOF
  > pick 177f92b77385 c
  > pick e860deea161a e
  > pick 652413bf663e f
  > pick 055a42cdd887 d
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
  @  changeset:   5:853c68da763f
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   4:26f6a030ae82
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   3:b069cc29fb22
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

  $ cat > $EDITED <<EOF
  > pick 177f92b77385 c
  > pick 853c68da763f d
  > pick b069cc29fb22 e
  > pick 26f6a030ae82 f
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
  

slightly different this time

  $ cat > $EDITED <<EOF
  > pick 055a42cdd887 d
  > pick 652413bf663e f
  > pick e860deea161a e
  > pick 177f92b77385 c
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log --graph
  @  changeset:   5:99a62755c625
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   4:7c6fdd608667
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:c4f52e213402
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:bfe4a5a76b37
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
  $ cat > $EDITED <<EOF
  > pick bfe4a5a76b37 d
  > pick c4f52e213402 f
  > pick 99a62755c625 c
  > pick 7c6fdd608667 e
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit bfe4a5a76b37 --keep 2>&1 | fixbundle
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log --graph
  > cat > $EDITED <<EOF
  > pick 7c6fdd608667 e
  > pick 99a62755c625 c
  > EOF
  @  changeset:   7:99e266581538
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   6:5ad36efb0653
  |  parent:      3:c4f52e213402
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  | o  changeset:   5:99a62755c625
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     c
  | |
  | o  changeset:   4:7c6fdd608667
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     e
  |
  o  changeset:   3:c4f52e213402
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:bfe4a5a76b37
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
  $ hg histedit --commands "$EDITED" --rev -2 2>&1 | fixbundle
  abort: may not use changesets other than the ones listed
  $ hg log --graph
  @  changeset:   7:99e266581538
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   6:5ad36efb0653
  |  parent:      3:c4f52e213402
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  | o  changeset:   5:99a62755c625
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     c
  | |
  | o  changeset:   4:7c6fdd608667
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     e
  |
  o  changeset:   3:c4f52e213402
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   2:bfe4a5a76b37
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
