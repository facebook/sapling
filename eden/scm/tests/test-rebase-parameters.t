#chg-compatible
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure mutation-norecord dummyssh
  $ enable rebase
  $ setconfig phases.publish=false ui.allowemptycommit=True
  $ readconfig <<EOF
  > [alias]
  > tglog = log -G --template "{node|short} '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a
  $ setconfig extensions.treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  $ setconfig treemanifest.server=True
  $ hg commit -qm "A"
  $ hg commit -qm "B"
  $ hg commit -qm "C"
  $ hg commit -qm "D"
  $ hg up -q .~3
  $ hg commit -qm "E"
  $ hg book E
  $ hg up -q .~1
  $ hg commit -qm "F"
  $ hg merge -q E
  $ hg book -d E
  $ hg commit -qm "G"
  $ hg up -q .^
  $ hg commit -qm "H"

  $ echo I > I
  $ hg ci -AmI
  adding I

  $ tglog
  @  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  │ o  319f51d6224e 'G'
  ╭─┤
  o │  971baba67099 'F'
  │ │
  │ o  0e89a44ca1b2 'E'
  ├─╯
  │ o  9da08f1f4bcc 'D'
  │ │
  │ o  9b96ea441fce 'C'
  │ │
  │ o  f68855660cff 'B'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..

Version with only two heads (to allow default destination to work)

  $ hg clone -q -u tip ssh://user@dummy/a a2heads -r 3 -r 8

These fail:

  $ hg clone -q -u tip ssh://user@dummy/a a0
  $ cd a0

  $ hg rebase -s 'desc(I)' -d 'desc(H)'
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

  $ hg rebase --base 'desc(G)'
  abort: branch 'default' has 3 heads - please rebase to an explicit rev
  (run 'hg heads .' to see heads)
  [255]

  $ hg rebase --rev 'desc(B) & !desc(B)' --dest 8
  empty "rev" revision set - nothing to rebase

  $ hg rebase --source 'desc(B) & !desc(B)' --dest 8
  empty "source" revision set - nothing to rebase

  $ hg rebase --base 'desc(B) & !desc(B)' --dest 8
  empty "base" revision set - can't compute rebase set

  $ hg rebase --dest 'desc(I)'
  nothing to rebase - working directory parent is also destination

  $ hg rebase -b . --dest 'desc(I)'
  nothing to rebase - 442d0c3a332a is both "base" and destination

  $ hg up -q 'desc(H)'

  $ hg rebase --dest 'desc(I)' --traceback
  nothing to rebase - working directory parent is already an ancestor of destination 442d0c3a332a

  $ hg rebase --dest 'desc(I)' -b.
  nothing to rebase - "base" 23a00112b28c is already an ancestor of destination 442d0c3a332a

  $ hg rebase --dest 'desc(B) & !desc(B)'
  abort: empty revision set
  [255]

These work:

Rebase with no arguments (from 3 onto 8):

  $ cd ..
  $ cp -R a2heads a1
  $ cd a1
  $ hg up -q -C 'desc(D)'

  $ hg rebase
  rebasing f68855660cff "B"
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  @  c691b1d40ebc 'D'
  │
  o  f5101d28cadd 'C'
  │
  o  4ff9cf5957f0 'B'
  │
  o  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  o  971baba67099 'F'
  │
  o  7b3f3d5e5faf 'A'
  
  $ cd ..

Rebase with base == '.' => same as no arguments (from 3 onto 8):

  $ cp -R a2heads a2
  $ cd a2
  $ hg goto -q 'desc(D)'

  $ hg rebase --base .
  rebasing f68855660cff "B"
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  @  c691b1d40ebc 'D'
  │
  o  f5101d28cadd 'C'
  │
  o  4ff9cf5957f0 'B'
  │
  o  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  o  971baba67099 'F'
  │
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Rebase with dest == branch(.) => same as no arguments (from 3 onto 8):

  $ hg clone -q -u 3 a a3
  $ cd a3

  $ hg rebase --dest 'branch(.)'
  rebasing f68855660cff "B"
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  @  c691b1d40ebc 'D'
  │
  o  f5101d28cadd 'C'
  │
  o  4ff9cf5957f0 'B'
  │
  o  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  │ o  319f51d6224e 'G'
  ╭─┤
  o │  971baba67099 'F'
  │ │
  │ o  0e89a44ca1b2 'E'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Specify only source (from 2 onto 8):

  $ cp -R a2heads a4
  $ cd a4

  $ hg rebase --source 'desc("C")'
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  o  eccbb0403c5f 'D'
  │
  o  e8496abab162 'C'
  │
  @  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  o  971baba67099 'F'
  │
  │ o  f68855660cff 'B'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Specify only dest (from 3 onto 6):

  $ hg clone -q -u 3 a a5
  $ cd a5

  $ hg rebase --dest 'desc(G)'
  rebasing f68855660cff "B"
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  @  600bb15de336 'D'
  │
  o  38c964e15e52 'C'
  │
  o  1c9fe805ca4e 'B'
  │
  │ o  442d0c3a332a 'I'
  │ │
  │ o  23a00112b28c 'H'
  │ │
  o │  319f51d6224e 'G'
  ├─╮
  │ o  971baba67099 'F'
  │ │
  o │  0e89a44ca1b2 'E'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Specify only base (from 1 onto 8):

  $ cp -R a2heads a6
  $ cd a6

  $ hg rebase --base 'desc("D")'
  rebasing f68855660cff "B"
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  o  c691b1d40ebc 'D'
  │
  o  f5101d28cadd 'C'
  │
  o  4ff9cf5957f0 'B'
  │
  @  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  o  971baba67099 'F'
  │
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Specify source and dest (from 2 onto 7):

  $ cp -R a a7
  $ cd a7

  $ hg rebase --source 'desc(C)' --dest 'desc(H)'
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  o  1106330f5f77 'D'
  │
  o  69c2c425e598 'C'
  │
  │ @  442d0c3a332a 'I'
  ├─╯
  o  23a00112b28c 'H'
  │
  │ o  319f51d6224e 'G'
  ╭─┤
  o │  971baba67099 'F'
  │ │
  │ o  0e89a44ca1b2 'E'
  ├─╯
  │ o  f68855660cff 'B'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Specify base and dest (from 1 onto 7):

  $ cp -R a a8
  $ cd a8

  $ hg rebase --base 'desc(D)' --dest 'desc(H)'
  rebasing f68855660cff "B"
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  o  98e5f65f4dfa 'D'
  │
  o  d8f5725d7e97 'C'
  │
  o  c3c3bee19f3c 'B'
  │
  │ @  442d0c3a332a 'I'
  ├─╯
  o  23a00112b28c 'H'
  │
  │ o  319f51d6224e 'G'
  ╭─┤
  o │  971baba67099 'F'
  │ │
  │ o  0e89a44ca1b2 'E'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..


Specify only revs (from 2 onto 8)

  $ cp -R a2heads a9
  $ cd a9

  $ hg rebase --rev 'desc("C")::'
  rebasing 9b96ea441fce "C"
  rebasing 9da08f1f4bcc "D"

  $ tglog
  o  eccbb0403c5f 'D'
  │
  o  e8496abab162 'C'
  │
  @  442d0c3a332a 'I'
  │
  o  23a00112b28c 'H'
  │
  o  971baba67099 'F'
  │
  │ o  f68855660cff 'B'
  ├─╯
  o  7b3f3d5e5faf 'A'
  
  $ cd ..

Rebasing both a single revision and a merge in one command

  $ cp -R a aX
  $ cd aX
  $ hg rebase -r 9da08f1f4bcc -r 6 --dest 'desc(I)'
  rebasing 9da08f1f4bcc "D"
  rebasing 319f51d6224e "G"
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

  $ hg up -q 'desc(c1)'
  $ echo c2b > c2
  $ hg ci -Am c2b
  adding c2

  $ cd ..

  $ cp -R b b1
  $ cd b1

  $ hg rebase -s 'desc(c2b)' -d 56daeba07f4b2d0735ba0d40955813b42b4e4a4b --tool internal:local
  rebasing e4e3f3546619 "c2b"

  $ hg cat c2
  c2

  $ cd ..


  $ cp -R b b2
  $ cd b2

  $ hg rebase -s 'desc(c2b)' -d 56daeba07f4b2d0735ba0d40955813b42b4e4a4b --tool internal:other
  rebasing e4e3f3546619 "c2b"

  $ hg cat c2
  c2b

  $ cd ..


  $ cp -R b b3
  $ cd b3

  $ hg rebase -s 'desc(c2b)' -d 56daeba07f4b2d0735ba0d40955813b42b4e4a4b --tool internal:fail
  rebasing e4e3f3546619 "c2b"
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg summary
  parent: 56daeba07f4b 
   c2
  parent: e4e3f3546619 
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
  rebasing e4e3f3546619 "c2b"

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
  $ hg rebase -d 'desc(a)'
  nothing to rebase from d7486e00c6f1 to 3903775176ed
  $ cd ..
