#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ configure mutation-norecord
  $ enable rebase
  $ setconfig phases.publish=false

  $ hg init a
  $ cd a

  $ touch .hg/rebasestate
  $ hg sum
  parent: 000000000000  (empty repository)
  commit: (clean)
  abort: .hg/rebasestate is incomplete
  [255]
  $ rm .hg/rebasestate

  $ echo c1 > common
  $ hg add common
  $ hg ci -m C1

  $ echo c2 >> common
  $ hg ci -m C2

  $ echo c3 >> common
  $ hg ci -m C3

  $ hg up -q -C 'desc(C2)'

  $ echo l1 >> extra
  $ hg add extra
  $ hg ci -m L1

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ hg ci -m L2

  $ tglogp
  @  46f0b057b5c0 draft 'L2'
  │
  o  3163e20567cc draft 'L1'
  │
  │ o  a9ce13b75fb5 draft 'C3'
  ├─╯
  o  11eb9c356adf draft 'C2'
  │
  o  178f1774564f draft 'C1'
  

Conflicting rebase:

  $ hg rebase -s 'desc(L1)' -d 'desc(C3)'
  rebasing 3163e20567cc "L1"
  rebasing 46f0b057b5c0 "L2"
  merging common
  warning: 1 conflicts while merging common! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Insert unsupported advisory merge record:

  $ hg --config extensions.fakemergerecord=$TESTDIR/fakemergerecord.py fakemergerecord -x
  $ hg debugmergestate
  * version 2 records
  local: 3e046f2ecedb793b97ed32108086edd1a162f8bc
  other: 46f0b057b5c061d276b91491c22151f78698abd2
  labels:
    local: dest
    other: source
  unrecognized entry: x	advisory record
  file extras: common (ancestorlinknode = 3163e20567cc93074fbb7a53c8b93312e59dbf2c)
  file: common (record type "F", state "u", hash 94c8c21d08740f5da9eaa38d1f175c592692f0d1)
    local path: common (flags "")
    ancestor path: common (node de0a666fdd9c1a0b0698b90d85064d8bd34f74b6)
    other path: common (node 2f6411de53677f6f1048fef5bf888d67a342e0a5)
  $ hg resolve -l
  U common

Insert unsupported mandatory merge record:

  $ hg --config extensions.fakemergerecord=$TESTDIR/fakemergerecord.py fakemergerecord -X
  $ hg debugmergestate
  * version 2 records
  local: 3e046f2ecedb793b97ed32108086edd1a162f8bc
  other: 46f0b057b5c061d276b91491c22151f78698abd2
  labels:
    local: dest
    other: source
  file extras: common (ancestorlinknode = 3163e20567cc93074fbb7a53c8b93312e59dbf2c)
  file: common (record type "F", state "u", hash 94c8c21d08740f5da9eaa38d1f175c592692f0d1)
    local path: common (flags "")
    ancestor path: common (node de0a666fdd9c1a0b0698b90d85064d8bd34f74b6)
    other path: common (node 2f6411de53677f6f1048fef5bf888d67a342e0a5)
  unrecognized entry: X	mandatory record
  $ hg resolve -l
  abort: unsupported merge state records: X
  (see https://mercurial-scm.org/wiki/MergeStateRecords for more information)
  [255]
  $ hg resolve -ma
  abort: unsupported merge state records: X
  (see https://mercurial-scm.org/wiki/MergeStateRecords for more information)
  [255]

Abort (should clear out unsupported merge state):

  $ hg rebase --abort
  rebase aborted
  $ hg debugmergestate
  no merge state found

  $ tglogp
  @  46f0b057b5c0 draft 'L2'
  │
  o  3163e20567cc draft 'L1'
  │
  │ o  a9ce13b75fb5 draft 'C3'
  ├─╯
  o  11eb9c356adf draft 'C2'
  │
  o  178f1774564f draft 'C1'
  
Test safety for inconsistent rebase state, which may be created (and
forgotten) by Mercurial earlier than 2.7. This emulates Mercurial
earlier than 2.7 by renaming ".hg/rebasestate" temporarily.

  $ hg rebase -s 3163e20567cc93074fbb7a53c8b93312e59dbf2c -d 'desc(C3)'
  rebasing 3163e20567cc "L1"
  rebasing 46f0b057b5c0 "L2"
  merging common
  warning: 1 conflicts while merging common! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ mv .hg/rebasestate .hg/rebasestate.back
  $ hg goto --quiet --clean 'desc(C3)'
  $ hg debugstrip --quiet "tip"
  $ mv .hg/rebasestate.back .hg/rebasestate

  $ hg rebase --continue
  abort: cannot continue inconsistent rebase
  (use "hg rebase --abort" to clear broken state)
  [255]
  $ hg summary | grep '^rebase: '
  rebase: (use "hg rebase --abort" to clear broken state)
  $ hg rebase --abort
  rebase aborted (no revision is removed, only broken state is cleared)

  $ cd ..


Construct new repo:

  $ hg init b
  $ cd b

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ echo c > c
  $ hg ci -Am C
  adding c

  $ hg up -q 'desc(A)'

  $ echo b > b
  $ hg ci -Am 'B bis'
  adding b

  $ echo c1 > c
  $ hg ci -Am C1
  adding c

  $ hg debugmakepublic 6c81ed0049f86eccdfa07f4d71b328a6c970b13f

Rebase and abort without generating new changesets:

  $ tglogp
  @  145842775fec draft 'C1'
  │
  o  a6484957d6b9 draft 'B bis'
  │
  │ o  49cb3485fa0c draft 'C'
  │ │
  │ o  6c81ed0049f8 public 'B'
  ├─╯
  o  1994f17a630e public 'A'
  
  $ hg rebase -b 'desc(C1)' -d 49cb3485fa0c1934763ac434487005741b74316f
  rebasing a6484957d6b9 "B bis"
  note: rebase of a6484957d6b9 created no changes to commit
  rebasing 145842775fec "C1"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ tglogp
  @  145842775fec draft 'C1'
  │
  o  a6484957d6b9 draft 'B bis'
  │
  │ @  49cb3485fa0c draft 'C'
  │ │
  │ o  6c81ed0049f8 public 'B'
  ├─╯
  o  1994f17a630e public 'A'
  
  $ hg rebase -a
  rebase aborted

  $ tglogp
  @  145842775fec draft 'C1'
  │
  o  a6484957d6b9 draft 'B bis'
  │
  │ o  49cb3485fa0c draft 'C'
  │ │
  │ o  6c81ed0049f8 public 'B'
  ├─╯
  o  1994f17a630e public 'A'
  

  $ cd ..

rebase abort should not leave working copy in a merge state if tip-1 is public
(issue4082)

  $ hg init abortpublic
  $ cd abortpublic
  $ echo a > a && hg ci -Aqm a
  $ hg book master
  $ hg book foo
  $ echo b > b && hg ci -Aqm b
  $ hg up -q master
  $ echo c > c && hg ci -Aqm c
  $ hg debugmakepublic -r .
  $ hg up -q foo
  $ echo C > c && hg ci -Aqm C
  $ hg log -G --template "{desc} {bookmarks}"
  @  C foo
  │
  o  b
  │
  │ o  c master
  ├─╯
  o  a
  

  $ hg rebase -d master -r foo
  rebasing 6c0f977a22d8 "C" (foo)
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg log -G --template "{desc} {bookmarks}"
  @  C foo
  │
  o  b
  │
  │ o  c master
  ├─╯
  o  a
  
  $ cd ..

Make sure we don't clobber changes in the working directory when the
user has somehow managed to update to a different revision (issue4009)

  $ hg init noupdate
  $ cd noupdate
  $ hg book @
  $ echo original > a
  $ hg add a
  $ hg commit -m a
  $ echo x > b
  $ hg add b
  $ hg commit -m b1
  $ hg up 'desc(a)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark @)
  $ hg book foo
  $ echo y > b
  $ hg add b
  $ hg commit -m b2

  $ hg rebase -d @ -b foo --tool=internal:fail
  rebasing 070cf4580bb5 "b2" (foo)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ mv .hg/rebasestate ./ # so we're allowed to hg up like in mercurial <2.6.3
  $ hg up -C 'desc(a)'            # user does other stuff in the repo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ mv rebasestate .hg/   # user upgrades to 2.7

  $ echo new > a
  $ hg up 1               # user gets an error saying to run hg rebase --abort
  abort: rebase in progress
  (use 'hg rebase --continue' or 'hg rebase --abort')
  [255]

  $ cat a
  new
  $ hg rebase --abort
  rebase aborted
  $ cat a
  new

  $ cd ..

test aborting an interrupted series (issue5084)
  $ hg init interrupted
  $ cd interrupted
  $ touch base
  $ hg add base
  $ hg commit -m base
  $ touch a
  $ hg add a
  $ hg commit -m a
  $ echo 1 > a
  $ hg commit -m 1
  $ touch b
  $ hg add b
  $ hg commit -m b
  $ echo 2 >> a
  $ hg commit -m c
  $ touch d
  $ hg add d
  $ hg commit -m d
  $ hg co -q 'max(desc(a))'
  $ hg rm a
  $ hg commit -m no-a
  $ hg co 'desc(base)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G --template "{desc} {bookmarks}"
  o  no-a
  │
  │ o  d
  │ │
  │ o  c
  │ │
  │ o  b
  │ │
  │ o  1
  ├─╯
  o  a
  │
  @  base
  
  $ hg --config extensions.n=$TESTDIR/failfilemerge.py rebase -s 'max(desc(b))' -d tip
  rebasing 3a71550954f1 "b"
  rebasing e80b69427d80 "c"
  abort: ^C
  [255]
  $ hg rebase --abort
  rebase aborted
  $ hg log -G --template "{desc} {bookmarks}"
  o  no-a
  │
  │ o  d
  │ │
  │ o  c
  │ │
  │ o  b
  │ │
  │ o  1
  ├─╯
  o  a
  │
  @  base
  
  $ hg summary
  parent: df4f53cec30a 
   base
  commit: (clean)
  phases: 7 draft

  $ cd ..
On the other hand, make sure we *do* clobber changes whenever we
haven't somehow managed to update the repo to a different revision
during a rebase (issue4661)

  $ hg ini yesupdate
  $ cd yesupdate
  $ echo "initial data" > foo.txt
  $ hg add
  adding foo.txt
  $ hg ci -m "initial checkin"
  $ echo "change 1" > foo.txt
  $ hg ci -m "change desc(change)"
  $ hg up 'desc(initial)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "conflicting change 1" > foo.txt
  $ hg ci -m "conflicting 1"
  $ echo "conflicting change 2" > foo.txt
  $ hg ci -m "conflicting 2"

  $ hg rebase -d 'desc(change)' --tool 'internal:fail'
  rebasing e4ea5cdc9789 "conflicting 1"
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg summary
  parent: b16646383533 
   conflicting 2
  commit: (clean)
  phases: 4 draft
  $ cd ..

test aborting a rebase succeeds after rebasing with skipped commits onto a
public changeset (issue4896)

  $ hg init succeedonpublic
  $ cd succeedonpublic
  $ echo 'content' > root
  $ hg commit -A -m 'root' -q

set up public branch
  $ echo 'content' > disappear
  $ hg commit -A -m 'disappear public' -q
commit will cause merge conflict on rebase
  $ echo '' > root
  $ hg commit -m 'remove content public' -q
  $ hg debugmakepublic

setup the draft branch that will be rebased onto public commit
  $ hg up -r 'desc(root)' -q
  $ echo 'content' > disappear
commit will disappear
  $ hg commit -A -m 'disappear draft' -q
  $ echo 'addedcontADDEDentadded' > root
commit will cause merge conflict on rebase
  $ hg commit -m 'add content draft' -q

  $ hg rebase -d 'public()' --tool :merge -q
  note: rebase of 0682fd3dabf5 created no changes to commit
  warning: 1 conflicts while merging root! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ cd ..

