  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > histedit=
  > EOF

  $ initrepo ()
  > {
  >     hg init $1
  >     cd $1
  >     for x in a b c d e f ; do
  >         echo $x$x$x$x$x > $x
  >         hg add $x
  >     done
  >     hg ci -m 'Initial commit'
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg ci -m $x
  >     done
  >     echo 'I can haz no commute' > e
  >     hg ci -m 'does not commute with e'
  >     cd ..
  > }

  $ initrepo r
  $ cd r
Initial generation of the command files

  $ EDITED="$TESTTMP/editedhistory"
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 3 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 4 >> $EDITED
  $ hg log --template 'fold {node|short} {rev} {desc}\n' -r 7 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 5 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 6 >> $EDITED
  $ cat $EDITED
  pick 65a9a84f33fd 3 c
  pick 00f1c5383965 4 d
  fold 39522b764e3d 7 does not commute with e
  pick 7b4e2f4b7bcd 5 e
  pick 500cac37a696 6 f

log before edit
  $ hg log --graph
  @  changeset:   7:39522b764e3d
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     does not commute with e
  |
  o  changeset:   6:500cac37a696
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   5:7b4e2f4b7bcd
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   4:00f1c5383965
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   3:65a9a84f33fd
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   2:da6535b52e45
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   1:c1f09da44841
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     a
  |
  o  changeset:   0:1715188a53c7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     Initial commit
  

edit the history
  $ hg histedit 3 --commands $EDITED 2>&1 | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging e
  warning: conflicts during merge.
  merging e incomplete! (edit conflicts, then use 'hg resolve --mark')
  Fix up the change and run hg histedit --continue

fix up
  $ echo 'I can haz no commute' > e
  $ hg resolve --mark e
  $ cat > cat.py <<EOF
  > import sys
  > print open(sys.argv[1]).read()
  > print
  > print
  > EOF
  $ HGEDITOR="python cat.py" hg histedit --continue 2>&1 | fixbundle | grep -v '2 files removed'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  d
  ***
  does not commute with e
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed d
  HG: changed e
  
  
  
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging e
  warning: conflicts during merge.
  merging e incomplete! (edit conflicts, then use 'hg resolve --mark')
  Fix up the change and run hg histedit --continue

just continue this time
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  $ hg histedit --continue 2>&1 | fixbundle
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

log after edit
  $ hg log --graph
  @  changeset:   5:d9cf42e54966
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:10486af2e984
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   3:65a9a84f33fd
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   2:da6535b52e45
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   1:c1f09da44841
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     a
  |
  o  changeset:   0:1715188a53c7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     Initial commit
  

contents of e
  $ hg cat e
  I can haz no commute

manifest
  $ hg manifest
  a
  b
  c
  d
  e
  f

  $ cd ..
