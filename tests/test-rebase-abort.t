  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a

  $ touch .hg/rebasestate
  $ hg sum
  parent: -1:000000000000 tip (empty repository)
  branch: default
  commit: (clean)
  update: (current)
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

  $ hg up -q -C 1

  $ echo l1 >> extra
  $ hg add extra
  $ hg ci -m L1
  created new head

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ hg ci -m L2

  $ hg phase --force --secret 2

  $ hg tglog
  @  4:draft 'L2'
  |
  o  3:draft 'L1'
  |
  | o  2:secret 'C3'
  |/
  o  1:draft 'C2'
  |
  o  0:draft 'C1'
  

Conflicting rebase:

  $ hg rebase -s 3 -d 2
  rebasing 3:3163e20567cc "L1"
  rebasing 4:46f0b057b5c0 "L2" (tip)
  merging common
  warning: conflicts during merge.
  merging common incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Abort:

  $ hg rebase --abort
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/3e046f2ecedb-6beef7d5-backup.hg (glob)
  rebase aborted

  $ hg tglog
  @  4:draft 'L2'
  |
  o  3:draft 'L1'
  |
  | o  2:secret 'C3'
  |/
  o  1:draft 'C2'
  |
  o  0:draft 'C1'
  
Test safety for inconsistent rebase state, which may be created (and
forgotten) by Mercurial earlier than 2.7. This emulates Mercurial
earlier than 2.7 by renaming ".hg/rebasestate" temporarily.

  $ hg rebase -s 3 -d 2
  rebasing 3:3163e20567cc "L1"
  rebasing 4:46f0b057b5c0 "L2" (tip)
  merging common
  warning: conflicts during merge.
  merging common incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ mv .hg/rebasestate .hg/rebasestate.back
  $ hg update --quiet --clean 2
  $ hg --config extensions.mq= strip --quiet "destination()"
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

  $ hg up -q 0

  $ echo b > b
  $ hg ci -Am 'B bis'
  adding b
  created new head

  $ echo c1 > c
  $ hg ci -Am C1
  adding c

  $ hg phase --force --secret 1
  $ hg phase --public 1

Rebase and abort without generating new changesets:

  $ hg tglog
  @  4:draft 'C1'
  |
  o  3:draft 'B bis'
  |
  | o  2:secret 'C'
  | |
  | o  1:public 'B'
  |/
  o  0:public 'A'
  
  $ hg rebase -b 4 -d 2
  rebasing 3:a6484957d6b9 "B bis"
  note: rebase of 3:a6484957d6b9 created no changes to commit
  rebasing 4:145842775fec "C1" (tip)
  merging c
  warning: conflicts during merge.
  merging c incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg tglog
  @  4:draft 'C1'
  |
  o  3:draft 'B bis'
  |
  | @  2:secret 'C'
  | |
  | o  1:public 'B'
  |/
  o  0:public 'A'
  
  $ hg rebase -a
  rebase aborted

  $ hg tglog
  @  4:draft 'C1'
  |
  o  3:draft 'B bis'
  |
  | o  2:secret 'C'
  | |
  | o  1:public 'B'
  |/
  o  0:public 'A'
  

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
  $ hg phase -p -r .
  $ hg up -q foo
  $ echo C > c && hg ci -Aqm C
  $ hg log -G --template "{rev} {desc} {bookmarks}"
  @  3 C foo
  |
  | o  2 c master
  | |
  o |  1 b
  |/
  o  0 a
  

  $ hg rebase -d master -r foo
  rebasing 3:6c0f977a22d8 "C" (tip foo)
  merging c
  warning: conflicts during merge.
  merging c incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg log -G --template "{rev} {desc} {bookmarks}"
  @  3 C foo
  |
  | o  2 c master
  | |
  o |  1 b
  |/
  o  0 a
  
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
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark @)
  $ hg book foo
  $ echo y > b
  $ hg add b
  $ hg commit -m b2
  created new head

  $ hg rebase -d @ -b foo --tool=internal:fail
  rebasing 2:070cf4580bb5 "b2" (tip foo)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ mv .hg/rebasestate ./ # so we're allowed to hg up like in mercurial <2.6.3
  $ hg up -C 0            # user does other stuff in the repo
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
  $ hg ci -m "change 1"
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "conflicting change 1" > foo.txt
  $ hg ci -m "conflicting 1"
  created new head
  $ echo "conflicting change 2" > foo.txt
  $ hg ci -m "conflicting 2"

  $ hg rebase -d 1 --tool 'internal:fail'
  rebasing 2:e4ea5cdc9789 "conflicting 1"
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg summary
  parent: 3:b16646383533 tip
   conflicting 2
  branch: default
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 4 draft
