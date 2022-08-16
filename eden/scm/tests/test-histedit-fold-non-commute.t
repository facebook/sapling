#chg-compatible
#debugruntest-compatible

  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable histedit

  $ modwithdate ()
  > {
  >     echo $1 > $1
  >     hg ci -m $1 -d "$2 0"
  > }

  $ initrepo ()
  > {
  >     hg init $1
  >     cd $1
  >     for x in a b c d e f ; do
  >         echo $x$x$x$x$x > $x
  >         hg add $x
  >     done
  >     hg ci -m 'Initial commit'
  >     modwithdate a 1
  >     modwithdate b 2
  >     modwithdate c 3
  >     modwithdate d 4
  >     modwithdate e 5
  >     modwithdate f 6
  >     echo 'I can haz no commute' > e
  >     hg ci -m 'does not commute with e' -d '7 0'
  >     cd ..
  > }

  $ initrepo r
  $ cd r
Initial generation of the command files

  $ EDITED="$TESTTMP/editedhistory"
  $ hg log --template 'pick {node|short} {desc}\n' -r 092e4ce14829f4974399ce4316d59f64ef0b6725 >> $EDITED
  $ hg log --template 'pick {node|short} {desc}\n' -r ae78f4c9d74ffa4b6cb5045001c303fe9204e890 >> $EDITED
  $ hg log --template 'fold {node|short} {desc}\n' -r 'desc(does)' >> $EDITED
  $ hg log --template 'pick {node|short} {desc}\n' -r 7f3755409b009372829d4180bfecef5c5bb3a10e >> $EDITED
  $ hg log --template 'pick {node|short} {desc}\n' -r 'desc(f)' >> $EDITED
  $ cat $EDITED
  pick 092e4ce14829 c
  pick ae78f4c9d74f d
  fold 42abbb61bede does not commute with e
  pick 7f3755409b00 e
  pick dd184f2faeb0 f

log before edit
  $ hg log --graph
  @  commit:      42abbb61bede
  │  user:        test
  │  date:        Thu Jan 01 00:00:07 1970 +0000
  │  summary:     does not commute with e
  │
  o  commit:      dd184f2faeb0
  │  user:        test
  │  date:        Thu Jan 01 00:00:06 1970 +0000
  │  summary:     f
  │
  o  commit:      7f3755409b00
  │  user:        test
  │  date:        Thu Jan 01 00:00:05 1970 +0000
  │  summary:     e
  │
  o  commit:      ae78f4c9d74f
  │  user:        test
  │  date:        Thu Jan 01 00:00:04 1970 +0000
  │  summary:     d
  │
  o  commit:      092e4ce14829
  │  user:        test
  │  date:        Thu Jan 01 00:00:03 1970 +0000
  │  summary:     c
  │
  o  commit:      40ccdd8beb95
  │  user:        test
  │  date:        Thu Jan 01 00:00:02 1970 +0000
  │  summary:     b
  │
  o  commit:      cd997a145b29
  │  user:        test
  │  date:        Thu Jan 01 00:00:01 1970 +0000
  │  summary:     a
  │
  o  commit:      1715188a53c7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     Initial commit
  

edit the history
  $ hg histedit 092e4ce14829f4974399ce4316d59f64ef0b6725 --commands $EDITED 2>&1 | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging e
  warning: 1 conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (fold 42abbb61bede)
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
  warning: 1 conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 7f3755409b00)
  (hg histedit --continue to resume)

just continue this time
keep the non-commuting change, and thus the pending change will be dropped
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg diff
  $ hg histedit --continue 2>&1 | fixbundle
  7f3755409b00: skipping changeset (no changes)

log after edit
  $ hg log --graph
  @  commit:      1300355b1a54
  │  user:        test
  │  date:        Thu Jan 01 00:00:06 1970 +0000
  │  summary:     f
  │
  o  commit:      e2ac33269083
  │  user:        test
  │  date:        Thu Jan 01 00:00:07 1970 +0000
  │  summary:     d
  │
  o  commit:      092e4ce14829
  │  user:        test
  │  date:        Thu Jan 01 00:00:03 1970 +0000
  │  summary:     c
  │
  o  commit:      40ccdd8beb95
  │  user:        test
  │  date:        Thu Jan 01 00:00:02 1970 +0000
  │  summary:     b
  │
  o  commit:      cd997a145b29
  │  user:        test
  │  date:        Thu Jan 01 00:00:01 1970 +0000
  │  summary:     a
  │
  o  commit:      1715188a53c7
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

Repeat test using "roll", not "fold". "roll" folds in changes but drops message and date

  $ initrepo r2
  $ cd r2

Initial generation of the command files

  $ EDITED="$TESTTMP/editedhistory.2"
  $ hg log --template 'pick {node|short} {desc}\n' -r 092e4ce14829f4974399ce4316d59f64ef0b6725 >> $EDITED
  $ hg log --template 'pick {node|short} {desc}\n' -r ae78f4c9d74ffa4b6cb5045001c303fe9204e890 >> $EDITED
  $ hg log --template 'roll {node|short} {desc}\n' -r 'desc(does)' >> $EDITED
  $ hg log --template 'pick {node|short} {desc}\n' -r 7f3755409b009372829d4180bfecef5c5bb3a10e >> $EDITED
  $ hg log --template 'pick {node|short} {desc}\n' -r 'desc(f)' >> $EDITED
  $ cat $EDITED
  pick 092e4ce14829 c
  pick ae78f4c9d74f d
  roll 42abbb61bede does not commute with e
  pick 7f3755409b00 e
  pick dd184f2faeb0 f

log before edit
  $ hg log --graph
  @  commit:      42abbb61bede
  │  user:        test
  │  date:        Thu Jan 01 00:00:07 1970 +0000
  │  summary:     does not commute with e
  │
  o  commit:      dd184f2faeb0
  │  user:        test
  │  date:        Thu Jan 01 00:00:06 1970 +0000
  │  summary:     f
  │
  o  commit:      7f3755409b00
  │  user:        test
  │  date:        Thu Jan 01 00:00:05 1970 +0000
  │  summary:     e
  │
  o  commit:      ae78f4c9d74f
  │  user:        test
  │  date:        Thu Jan 01 00:00:04 1970 +0000
  │  summary:     d
  │
  o  commit:      092e4ce14829
  │  user:        test
  │  date:        Thu Jan 01 00:00:03 1970 +0000
  │  summary:     c
  │
  o  commit:      40ccdd8beb95
  │  user:        test
  │  date:        Thu Jan 01 00:00:02 1970 +0000
  │  summary:     b
  │
  o  commit:      cd997a145b29
  │  user:        test
  │  date:        Thu Jan 01 00:00:01 1970 +0000
  │  summary:     a
  │
  o  commit:      1715188a53c7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     Initial commit
  

edit the history
  $ hg histedit 092e4ce14829f4974399ce4316d59f64ef0b6725 --commands $EDITED 2>&1 | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging e
  warning: 1 conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (roll 42abbb61bede)
  (hg histedit --continue to resume)

fix up
  $ echo 'I can haz no commute' > e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle | grep -v '2 files removed'
  merging e
  warning: 1 conflicts while merging e! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 7f3755409b00)
  (hg histedit --continue to resume)

just continue this time
  $ hg revert -r 'p1()' e
  $ hg resolve --mark e
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue 2>&1 | fixbundle
  7f3755409b00: skipping changeset (no changes)

log after edit
  $ hg log --graph
  @  commit:      b538bcb461be
  │  user:        test
  │  date:        Thu Jan 01 00:00:06 1970 +0000
  │  summary:     f
  │
  o  commit:      317e37cb6d66
  │  user:        test
  │  date:        Thu Jan 01 00:00:04 1970 +0000
  │  summary:     d
  │
  o  commit:      092e4ce14829
  │  user:        test
  │  date:        Thu Jan 01 00:00:03 1970 +0000
  │  summary:     c
  │
  o  commit:      40ccdd8beb95
  │  user:        test
  │  date:        Thu Jan 01 00:00:02 1970 +0000
  │  summary:     b
  │
  o  commit:      cd997a145b29
  │  user:        test
  │  date:        Thu Jan 01 00:00:01 1970 +0000
  │  summary:     a
  │
  o  commit:      1715188a53c7
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

  $ hg log --debug --rev 'max(desc(d))'
  commit:      317e37cb6d66c1c84628c00e5bf4c8c292831951
  phase:       draft
  manifest:    b068a323d969f22af1296ec6a5ea9384cef437ac
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       d e
  extra:       branch=default
  extra:       histedit_source=ae78f4c9d74ffa4b6cb5045001c303fe9204e890,42abbb61bede6f4366fa1e74a664343e5d558a70
  description:
  d
  
  

done with repo r2

  $ cd ..
