  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
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
  merging common
  warning: conflicts during merge.
  merging common incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Abort:

  $ hg rebase --abort
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)
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
  (use "hg rebase --abort" to clear borken state)
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
