  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
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
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (fold 39522b764e3d)
  (hg histedit --continue to resume)

fix up
  $ echo 'I can haz no commute' > e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ HGEDITOR=cat hg histedit --continue 2>&1 | fixbundle | grep -v '2 files removed'
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
  merging e
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 7b4e2f4b7bcd)
  (hg histedit --continue to resume)

just continue this time
keep the non-commuting change, and thus the pending change will be dropped
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg diff
  $ hg histedit --continue 2>&1 | fixbundle
  7b4e2f4b7bcd: skipping changeset (no changes)

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

Repeat test using "roll", not "fold". "roll" folds in changes but drops message

  $ initrepo r2
  $ cd r2

Initial generation of the command files

  $ EDITED="$TESTTMP/editedhistory.2"
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 3 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 4 >> $EDITED
  $ hg log --template 'roll {node|short} {rev} {desc}\n' -r 7 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 5 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 6 >> $EDITED
  $ cat $EDITED
  pick 65a9a84f33fd 3 c
  pick 00f1c5383965 4 d
  roll 39522b764e3d 7 does not commute with e
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
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (roll 39522b764e3d)
  (hg histedit --continue to resume)

fix up
  $ echo 'I can haz no commute' > e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle | grep -v '2 files removed'
  merging e
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 7b4e2f4b7bcd)
  (hg histedit --continue to resume)

just continue this time
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle
  7b4e2f4b7bcd: skipping changeset (no changes)

log after edit
  $ hg log --graph
  @  changeset:   5:e7c4f5d4eb75
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:803d1bb561fc
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

description is taken from rollup target commit

  $ hg log --debug --rev 4
  changeset:   4:803d1bb561fceac3129ec778db9da249a3106fc3
  phase:       draft
  parent:      3:65a9a84f33fdeb1ad5679b3941ec885d2b24027b
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4:b068a323d969f22af1296ec6a5ea9384cef437ac
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       d e
  extra:       branch=default
  extra:       histedit_source=00f1c53839651fa5c76d423606811ea5455a79d0,39522b764e3d26103f08bd1fa2ccd3e3d7dbcf4e
  description:
  d
  
  

done with repo r2

  $ cd ..
