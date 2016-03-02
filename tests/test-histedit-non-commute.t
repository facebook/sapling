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

  $ initrepo r1
  $ cd r1

Initial generation of the command files

  $ EDITED="$TESTTMP/editedhistory"
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 3 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 4 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 7 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 5 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 6 >> $EDITED
  $ cat $EDITED
  pick 65a9a84f33fd 3 c
  pick 00f1c5383965 4 d
  pick 39522b764e3d 7 does not commute with e
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
  Fix up the change (pick 39522b764e3d)
  (hg histedit --continue to resume)

abort the edit
  $ hg histedit --abort 2>&1 | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved


second edit set

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
  Fix up the change (pick 39522b764e3d)
  (hg histedit --continue to resume)

fix up
  $ echo 'I can haz no commute' > e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle
  merging e
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 7b4e2f4b7bcd)
  (hg histedit --continue to resume)

This failure is caused by 7b4e2f4b7bcd "e" not rebasing the non commutative
former children.

just continue this time
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle
  7b4e2f4b7bcd: skipping changeset (no changes)

log after edit
  $ hg log --graph
  @  changeset:   6:7efe1373e4bc
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   5:e334d87a1e55
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     does not commute with e
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
  

start over

  $ cd ..

  $ initrepo r2
  $ cd r2
  $ rm $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 3 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 4 >> $EDITED
  $ hg log --template 'mess {node|short} {rev} {desc}\n' -r 7 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 5 >> $EDITED
  $ hg log --template 'pick {node|short} {rev} {desc}\n' -r 6 >> $EDITED
  $ cat $EDITED
  pick 65a9a84f33fd 3 c
  pick 00f1c5383965 4 d
  mess 39522b764e3d 7 does not commute with e
  pick 7b4e2f4b7bcd 5 e
  pick 500cac37a696 6 f

edit the history, this time with a fold action
  $ hg histedit 3 --commands $EDITED 2>&1 | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging e
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (mess 39522b764e3d)
  (hg histedit --continue to resume)

  $ echo 'I can haz no commute' > e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle
  merging e
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 7b4e2f4b7bcd)
  (hg histedit --continue to resume)
second edit also fails, but just continue
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle
  7b4e2f4b7bcd: skipping changeset (no changes)

post message fix
  $ hg log --graph
  @  changeset:   6:7efe1373e4bc
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   5:e334d87a1e55
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     does not commute with e
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
  

  $ cd ..
