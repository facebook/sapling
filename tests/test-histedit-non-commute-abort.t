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
  >     echo a >> e
  >     hg ci -m 'does not commute with e'
  >     cd ..
  > }

  $ initrepo
  $ cd r

log before edit
  $ hg log --graph
  @  changeset:   6:bfa474341cc9
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     does not commute with e
  |
  o  changeset:   5:652413bf663e
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
  

edit the history
  $ hg histedit 177f92b77385 --commands - 2>&1 <<EOF | fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick bfa474341cc9 does not commute with e
  > pick e860deea161a e
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  remote changed e which local deleted
  use (c)hanged version or leave (d)eleted? c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging e
  warning: conflicts during merge.
  merging e incomplete! (edit conflicts, then use 'hg resolve --mark')
  Fix up the change and run hg histedit --continue


abort the edit
  $ hg histedit --abort 2>&1 | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

log after abort
  $ hg resolve -l
  $ hg log --graph
  @  changeset:   6:bfa474341cc9
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     does not commute with e
  |
  o  changeset:   5:652413bf663e
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
  

  $ cd ..
