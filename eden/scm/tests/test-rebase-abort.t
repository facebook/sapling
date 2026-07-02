
  $ eagerepo
  $ configure mutation-norecord
  $ enable rebase
  $ setconfig phases.publish=false

  $ newclientrepo

  $ touch .sl/rebasestate
  $ sl sum
  parent: 000000000000  (empty repository)
  commit: (clean)
  abort: .sl/rebasestate is incomplete
  [255]
  $ rm .sl/rebasestate

  $ echo c1 > common
  $ sl add common
  $ sl ci -m C1

  $ echo c2 >> common
  $ sl ci -m C2

  $ echo c3 >> common
  $ sl ci -m C3

  $ sl up -q -C 'desc(C2)'

  $ echo l1 >> extra
  $ sl add extra
  $ sl ci -m L1

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ sl ci -m L2

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

  $ sl rebase -s 'desc(L1)' -d 'desc(C3)'
  rebasing 3163e20567cc "L1"
  rebasing 46f0b057b5c0 "L2"
  merging common
  warning: 1 conflicts while merging common! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

Insert unsupported advisory merge record:

  $ sl debugmergestate --add-unsupported-advisory-record
  $ sl debugmergestate
  local: 3e046f2ecedb793b97ed32108086edd1a162f8bc
  other: 46f0b057b5c061d276b91491c22151f78698abd2
  labels:
    local: dest
    other: source
    base: base
  file: common (record type "F", state "u", hash 94c8c21d08740f5da9eaa38d1f175c592692f0d1)
    local path: common (flags "")
    ancestor path: common (node de0a666fdd9c1a0b0698b90d85064d8bd34f74b6)
    other path: common (node 2f6411de53677f6f1048fef5bf888d67a342e0a5)
    extras: ancestorlinknode=3163e20567cc93074fbb7a53c8b93312e59dbf2c
  unsupported record "x" (data ["advisory record"])
  $ sl resolve -l
  U common

Insert unsupported mandatory merge record:

  $ sl debugmergestate --add-unsupported-mandatory-record
  $ sl debugmergestate
  local: 3e046f2ecedb793b97ed32108086edd1a162f8bc
  other: 46f0b057b5c061d276b91491c22151f78698abd2
  labels:
    local: dest
    other: source
    base: base
  file: common (record type "F", state "u", hash 94c8c21d08740f5da9eaa38d1f175c592692f0d1)
    local path: common (flags "")
    ancestor path: common (node de0a666fdd9c1a0b0698b90d85064d8bd34f74b6)
    other path: common (node 2f6411de53677f6f1048fef5bf888d67a342e0a5)
    extras: ancestorlinknode=3163e20567cc93074fbb7a53c8b93312e59dbf2c
  unsupported record "X" (data ["mandatory record"])
  $ sl resolve -l
  abort: unsupported merge state records: X
  (see https://mercurial-scm.org/wiki/MergeStateRecords for more information)
  [255]
  $ sl resolve -ma
  abort: unsupported merge state records: X
  (see https://mercurial-scm.org/wiki/MergeStateRecords for more information)
  [255]

Abort (should clear out unsupported merge state):

  $ sl rebase --abort
  rebase aborted
  $ sl debugmergestate
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
earlier than 2.7 by renaming ".sl/rebasestate" temporarily.

  $ sl rebase -s 3163e20567cc93074fbb7a53c8b93312e59dbf2c -d 'desc(C3)'
  rebasing 3163e20567cc "L1"
  rebasing 46f0b057b5c0 "L2"
  merging common
  warning: 1 conflicts while merging common! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

  $ mv .sl/rebasestate .sl/rebasestate.back
  $ sl goto --quiet --clean 'desc(C3)'
  $ sl debugstrip --quiet "tip"
  $ mv .sl/rebasestate.back .sl/rebasestate

  $ sl rebase --continue
  abort: cannot continue inconsistent rebase
  (use "sl rebase --abort" to clear broken state)
  [255]
  $ sl summary | grep '^rebase: '
  rebase: (use "sl rebase --abort" to clear broken state)
  $ sl rebase --abort
  rebase aborted (no revision is removed, only broken state is cleared)

  $ cd ..


Construct new repo:

  $ sl init b
  $ cd b

  $ echo a > a
  $ sl ci -Am A
  adding a

  $ echo b > b
  $ sl ci -Am B
  adding b

  $ echo c > c
  $ sl ci -Am C
  adding c

  $ sl up -q 'desc(A)'

  $ echo b > b
  $ sl ci -Am 'B bis'
  adding b

  $ echo c1 > c
  $ sl ci -Am C1
  adding c

  $ sl debugmakepublic 6c81ed0049f86eccdfa07f4d71b328a6c970b13f

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
  
  $ sl rebase -b 'desc(C1)' -d 49cb3485fa0c1934763ac434487005741b74316f
  rebasing a6484957d6b9 "B bis"
  note: not rebasing a6484957d6b9, its destination (rebasing onto) commit already has all its changes
  rebasing 145842775fec "C1"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
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
  
  $ sl rebase -a
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

  $ sl init abortpublic
  $ cd abortpublic
  $ echo a > a && sl ci -Aqm a
  $ sl book master
  $ sl book foo
  $ echo b > b && sl ci -Aqm b
  $ sl up -q master
  $ echo c > c && sl ci -Aqm c
  $ sl debugmakepublic -r .
  $ sl up -q foo
  $ echo C > c && sl ci -Aqm C
  $ sl log -G --template "{desc} {bookmarks}"
  @  C foo
  │
  o  b
  │
  │ o  c master
  ├─╯
  o  a
  

  $ sl rebase -d master -r foo
  rebasing 6c0f977a22d8 "C" (foo)
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ sl rebase --abort
  rebase aborted
  $ sl log -G --template "{desc} {bookmarks}"
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

  $ sl init noupdate
  $ cd noupdate
  $ sl book @
  $ echo original > a
  $ sl add a
  $ sl commit -m a
  $ echo x > b
  $ sl add b
  $ sl commit -m b1
  $ sl up 'desc(a)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark @)
  $ sl book foo
  $ echo y > b
  $ sl add b
  $ sl commit -m b2

  $ sl rebase -d @ -b foo --tool=internal:fail
  rebasing 070cf4580bb5 "b2" (foo)
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

  $ mv .sl/rebasestate ./ # so we're allowed to sl up like in mercurial <2.6.3
  $ sl up -C 'desc(a)'            # user does other stuff in the repo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ mv rebasestate .sl/   # user upgrades to 2.7

  $ echo new > a
  $ sl up 1               # user gets an error saying to run sl rebase --abort
  abort: rebase in progress
  (use 'sl rebase --continue' to continue or
       'sl rebase --abort' to abort)
  [255]

  $ cat a
  new
  $ sl rebase --abort
  rebase aborted
  $ cat a
  new

  $ cd ..

test aborting an interrupted series (issue5084)
  $ sl init interrupted
  $ cd interrupted
  $ touch base
  $ sl add base
  $ sl commit -m base
  $ touch a
  $ sl add a
  $ sl commit -m a
  $ echo 1 > a
  $ sl commit -m 1
  $ touch b
  $ sl add b
  $ sl commit -m b
  $ echo 2 >> a
  $ sl commit -m c
  $ touch d
  $ sl add d
  $ sl commit -m d
  $ sl co -q 'max(desc(a))'
  $ sl rm a
  $ sl commit -m no-a
  $ sl co 'desc(base)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl log -G --template "{desc} {bookmarks}"
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
  
  $ sl --config extensions.n=$TESTDIR/failfilemerge.py rebase -s 'max(desc(b))' -d tip
  rebasing 3a71550954f1 "b"
  rebasing e80b69427d80 "c"
  abort: ^C
  [255]
  $ sl rebase --abort
  rebase aborted
  $ sl log -G --template "{desc} {bookmarks}"
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
  
  $ sl summary
  parent: df4f53cec30a 
   base
  commit: (clean)
  phases: 7 draft

  $ cd ..
On the other hand, make sure we *do* clobber changes whenever we
haven't somehow managed to update the repo to a different revision
during a rebase (issue4661)

  $ sl init yesupdate
  $ cd yesupdate
  $ echo "initial data" > foo.txt
  $ sl add
  adding foo.txt
  $ sl ci -m "initial checkin"
  $ echo "change 1" > foo.txt
  $ sl ci -m "change desc(change)"
  $ sl up 'desc(initial)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "conflicting change 1" > foo.txt
  $ sl ci -m "conflicting 1"
  $ echo "conflicting change 2" > foo.txt
  $ sl ci -m "conflicting 2"

  $ sl rebase -d 'desc(change)' --tool 'internal:fail'
  rebasing e4ea5cdc9789 "conflicting 1"
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ sl rebase --abort
  rebase aborted
  $ sl summary
  parent: b16646383533 
   conflicting 2
  commit: (clean)
  phases: 4 draft
  $ cd ..

test aborting a rebase succeeds after rebasing with skipped commits onto a
public changeset (issue4896)

  $ sl init succeedonpublic
  $ cd succeedonpublic
  $ echo 'content' > root
  $ sl commit -A -m 'root' -q

set up public branch
  $ echo 'content' > disappear
  $ sl commit -A -m 'disappear public' -q
commit will cause merge conflict on rebase
  $ echo '' > root
  $ sl commit -m 'remove content public' -q
  $ sl debugmakepublic

setup the draft branch that will be rebased onto public commit
  $ sl up -r 'desc(root)' -q
  $ echo 'content' > disappear
commit will disappear
  $ sl commit -A -m 'disappear draft' -q
  $ echo 'addedcontADDEDentadded' > root
commit will cause merge conflict on rebase
  $ sl commit -m 'add content draft' -q

  $ sl rebase -d 'public()' --tool :merge -q
  note: not rebasing 0682fd3dabf5, its destination (rebasing onto) commit already has all its changes
  warning: 1 conflicts while merging root! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ sl rebase --abort
  rebase aborted
  $ cd ..
