  $ setconfig extensions.treemanifest=!
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}: {node|short} '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files
  new changesets cd010b8cd998:02de42196ebe
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo I > I
  $ hg ci -AmI
  adding I

  $ tglog
  @  8: e7ec4e813ba6 'I'
  |
  o  7: 02de42196ebe 'H'
  |
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..

Version with only two heads (to allow default destination to work)

  $ hg clone -q -u . a a2heads -r 3 -r 8

These fail:

  $ hg clone -q -u . a a0
  $ cd a0

  $ hg rebase -s 8 -d 7
  nothing to rebase

  $ hg rebase --continue --abort
  abort: cannot use both abort and continue
  [255]

  $ hg rebase --continue --collapse
  abort: cannot use collapse with continue or abort
  [255]

  $ hg rebase --continue --dest 4
  abort: abort and continue do not allow specifying revisions
  [255]

  $ hg rebase --base 5 --source 4
  abort: cannot specify both a source and a base
  [255]

  $ hg rebase --rev 5 --source 4
  abort: cannot specify both a revision and a source
  [255]
  $ hg rebase --base 5 --rev 4
  abort: cannot specify both a revision and a base
  [255]

  $ hg rebase --base 6
  abort: branch 'default' has 3 heads - please rebase to an explicit rev
  (run 'hg heads .' to see heads)
  [255]

  $ hg rebase --rev '1 & !1' --dest 8
  empty "rev" revision set - nothing to rebase

  $ hg rebase --source '1 & !1' --dest 8
  empty "source" revision set - nothing to rebase

  $ hg rebase --base '1 & !1' --dest 8
  empty "base" revision set - can't compute rebase set

  $ hg rebase --dest 8
  nothing to rebase - working directory parent is also destination

  $ hg rebase -b . --dest 8
  nothing to rebase - e7ec4e813ba6 is both "base" and destination

  $ hg up -q 7

  $ hg rebase --dest 8 --traceback
  nothing to rebase - working directory parent is already an ancestor of destination e7ec4e813ba6

  $ hg rebase --dest 8 -b.
  nothing to rebase - "base" 02de42196ebe is already an ancestor of destination e7ec4e813ba6

  $ hg rebase --dest '1 & !1'
  abort: empty revision set
  [255]

These work:

Rebase with no arguments (from 3 onto 8):

  $ cd ..
  $ hg clone -q -u . a2heads a1
  $ cd a1
  $ hg up -q -C 3

  $ hg rebase
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  @  9: ed65089c18f8 'D'
  |
  o  8: 7621bf1a2f17 'C'
  |
  o  7: 9430a62369c6 'B'
  |
  o  6: e7ec4e813ba6 'I'
  |
  o  5: 02de42196ebe 'H'
  |
  o  4: 24b6387c8c8c 'F'
  |
  o  0: cd010b8cd998 'A'
  
Try to rollback after a rebase (fail):

  $ hg rollback
  no rollback information available
  [1]

  $ cd ..

Rebase with base == '.' => same as no arguments (from 3 onto 8):

  $ hg clone -q -u 3 a2heads a2
  $ cd a2

  $ hg rebase --base .
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  @  9: ed65089c18f8 'D'
  |
  o  8: 7621bf1a2f17 'C'
  |
  o  7: 9430a62369c6 'B'
  |
  o  6: e7ec4e813ba6 'I'
  |
  o  5: 02de42196ebe 'H'
  |
  o  4: 24b6387c8c8c 'F'
  |
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Rebase with dest == branch(.) => same as no arguments (from 3 onto 8):

  $ hg clone -q -u 3 a a3
  $ cd a3

  $ hg rebase --dest 'branch(.)'
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  @  11: ed65089c18f8 'D'
  |
  o  10: 7621bf1a2f17 'C'
  |
  o  9: 9430a62369c6 'B'
  |
  o  8: e7ec4e813ba6 'I'
  |
  o  7: 02de42196ebe 'H'
  |
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Specify only source (from 2 onto 8):

  $ hg clone -q -u . a2heads a4
  $ cd a4

  $ hg rebase --source 'desc("C")'
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  o  8: 7726e9fd58f7 'D'
  |
  o  7: 72c8333623d0 'C'
  |
  @  6: e7ec4e813ba6 'I'
  |
  o  5: 02de42196ebe 'H'
  |
  o  4: 24b6387c8c8c 'F'
  |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Specify only dest (from 3 onto 6):

  $ hg clone -q -u 3 a a5
  $ cd a5

  $ hg rebase --dest 6
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  @  11: 8eeb3c33ad33 'D'
  |
  o  10: 2327fea05063 'C'
  |
  o  9: e4e5be0395b2 'B'
  |
  | o  8: e7ec4e813ba6 'I'
  | |
  | o  7: 02de42196ebe 'H'
  | |
  o |  6: eea13746799a 'G'
  |\|
  | o  5: 24b6387c8c8c 'F'
  | |
  o |  4: 9520eea781bc 'E'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Specify only base (from 1 onto 8):

  $ hg clone -q -u . a2heads a6
  $ cd a6

  $ hg rebase --base 'desc("D")'
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  o  9: ed65089c18f8 'D'
  |
  o  8: 7621bf1a2f17 'C'
  |
  o  7: 9430a62369c6 'B'
  |
  @  6: e7ec4e813ba6 'I'
  |
  o  5: 02de42196ebe 'H'
  |
  o  4: 24b6387c8c8c 'F'
  |
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Specify source and dest (from 2 onto 7):

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase --source 2 --dest 7
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  o  10: 668acadedd30 'D'
  |
  o  9: 09eb682ba906 'C'
  |
  | @  8: e7ec4e813ba6 'I'
  |/
  o  7: 02de42196ebe 'H'
  |
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Specify base and dest (from 1 onto 7):

  $ hg clone -q -u . a a8
  $ cd a8

  $ hg rebase --base 3 --dest 7
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  o  11: 287cc92ba5a4 'D'
  |
  o  10: 6824f610a250 'C'
  |
  o  9: 7c6027df6a99 'B'
  |
  | @  8: e7ec4e813ba6 'I'
  |/
  o  7: 02de42196ebe 'H'
  |
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


Specify only revs (from 2 onto 8)

  $ hg clone -q -u . a2heads a9
  $ cd a9

  $ hg rebase --rev 'desc("C")::'
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"

  $ tglog
  o  8: 7726e9fd58f7 'D'
  |
  o  7: 72c8333623d0 'C'
  |
  @  6: e7ec4e813ba6 'I'
  |
  o  5: 02de42196ebe 'H'
  |
  o  4: 24b6387c8c8c 'F'
  |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..

Rebasing both a single revision and a merge in one command

  $ hg clone -q -u . a aX
  $ cd aX
  $ hg rebase -r 3 -r 6 --dest 8
  rebasing 32af7686d403 "D"
  rebasing eea13746799a "G"
  $ cd ..

Test --tool parameter:

  $ hg init b
  $ cd b

  $ echo c1 > c1
  $ hg ci -Am c1
  adding c1

  $ echo c2 > c2
  $ hg ci -Am c2
  adding c2

  $ hg up -q 0
  $ echo c2b > c2
  $ hg ci -Am c2b
  adding c2

  $ cd ..

  $ hg clone -q -u . b b1
  $ cd b1

  $ hg rebase -s 2 -d 1 --tool internal:local
  rebasing e4e3f3546619 "c2b" (tip)
  note: rebase of 2:e4e3f3546619 created no changes to commit

  $ hg cat c2
  c2

  $ cd ..


  $ hg clone -q -u . b b2
  $ cd b2

  $ hg rebase -s 2 -d 1 --tool internal:other
  rebasing e4e3f3546619 "c2b" (tip)

  $ hg cat c2
  c2b

  $ cd ..


  $ hg clone -q -u . b b3
  $ cd b3

  $ hg rebase -s 2 -d 1 --tool internal:fail
  rebasing e4e3f3546619 "c2b" (tip)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg summary
  parent: 1:56daeba07f4b 
   c2
  parent: 2:e4e3f3546619 tip
   c2b
  commit: 1 modified, 1 unresolved (merge)
  phases: 3 draft
  rebase: 0 rebased, 1 remaining (rebase --continue)

  $ hg resolve -l
  U c2

  $ hg resolve -m c2
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg graft --continue
  abort: no graft in progress
  (continue: hg rebase --continue)
  [255]
  $ hg rebase -c --tool internal:fail
  rebasing e4e3f3546619 "c2b" (tip)
  note: rebase of 2:e4e3f3546619 created no changes to commit

  $ hg rebase -i
  abort: interactive history editing is supported by the 'histedit' extension (see "hg --config extensions.histedit= help -e histedit")
  [255]

  $ hg rebase --interactive
  abort: interactive history editing is supported by the 'histedit' extension (see "hg --config extensions.histedit= help -e histedit")
  [255]

  $ cd ..

No common ancestor

  $ hg init separaterepo
  $ cd separaterepo
  $ touch a
  $ hg commit -Aqm a
  $ hg up -q null
  $ touch b
  $ hg commit -Aqm b
  $ hg rebase -d 0
  nothing to rebase from d7486e00c6f1 to 3903775176ed
  $ cd ..
