#chg-compatible
  $ eagerepo
  $ configure mutation-norecord dummyssh
  $ enable rebase
  $ setconfig phases.publish=false ui.allowemptycommit=True
  $ readconfig <<EOF
  > [alias]
  > tglog = log -G --template "{node|short} '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a
  $ drawdag <<'EOS'
  > I   D # I/I = I
  > |   | # bookmark master = I
  > H G C
  > |/| |
  > F E B
  >  \|/
  >   A
  > EOS

  $ tglog
  o  f585351a92f8 'D'
  │
  │ o    c6001eacfde5 'G'
  │ ├─╮
  o │ │  26805aba1e60 'C'
  │ │ │
  │ │ o  7fb047a69f22 'E'
  │ │ │
  o │ │  112478962961 'B'
  ├───╯
  │ │ o  3e65a434aea7 'I' master
  │ │ │
  │ │ o  4ea5b230dea3 'H'
  │ ├─╯
  │ o  8908a377a434 'F'
  ├─╯
  o  426bada5c675 'A'
  

  $ cd ..

Version with only two heads (to allow default destination to work)

  $ newclientrepo a2heads test:a
  $ hg pull -r $D
  pulling from test:a
  searching for changes

These fail:

  $ newclientrepo a0 test:a
  $ hg pull -r $I -r $G -r $D
  pulling from test:a
  searching for changes

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
  nothing to rebase - 3e65a434aea7 is both "base" and destination

  $ hg up -q 'desc(H)'

  $ hg rebase --dest 'desc(I)' --traceback
  nothing to rebase - working directory parent is already an ancestor of destination 3e65a434aea7

  $ hg rebase --dest 'desc(I)' -b.
  nothing to rebase - "base" 4ea5b230dea3 is already an ancestor of destination 3e65a434aea7

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
  rebasing 112478962961 "B"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  @  9ddd1ab93e66 'D'
  │
  o  5c73b5224523 'C'
  │
  o  0f744df8b10a 'B'
  │
  o  3e65a434aea7 'I'
  │
  o  4ea5b230dea3 'H'
  │
  o  8908a377a434 'F'
  │
  o  426bada5c675 'A'
  
  $ cd ..

Rebase with base == '.' => same as no arguments (from 3 onto 8):

  $ cp -R a2heads a2
  $ cd a2
  $ hg goto -q 'desc(D)'

  $ hg rebase --base .
  rebasing 112478962961 "B"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  @  9ddd1ab93e66 'D'
  │
  o  5c73b5224523 'C'
  │
  o  0f744df8b10a 'B'
  │
  o  3e65a434aea7 'I'
  │
  o  4ea5b230dea3 'H'
  │
  o  8908a377a434 'F'
  │
  o  426bada5c675 'A'
  
  $ cd ..


Specify only source (from 2 onto 8):

  $ cp -R a2heads a4
  $ cd a4

  $ hg rebase --source 'desc("C")'
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  o  53b9858adc18 'D'
  │
  o  b2b0670adeee 'C'
  │
  │ o  112478962961 'B'
  │ │
  @ │  3e65a434aea7 'I'
  │ │
  o │  4ea5b230dea3 'H'
  │ │
  o │  8908a377a434 'F'
  ├─╯
  o  426bada5c675 'A'
  
  $ cd ..


Specify only dest (from 3 onto 6):

  $ cp -R a a5
  $ cd a5
  $ hg goto -q $D

  $ hg rebase --dest 'desc(G)'
  rebasing 112478962961 "B"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  @  fd9396964646 'D'
  │
  o  9fa31dc0e74e 'C'
  │
  o  566695ae5898 'B'
  │
  o    c6001eacfde5 'G'
  ├─╮
  │ o  7fb047a69f22 'E'
  │ │
  │ │ o  3e65a434aea7 'I' master
  │ │ │
  │ │ o  4ea5b230dea3 'H'
  ├───╯
  o │  8908a377a434 'F'
  ├─╯
  o  426bada5c675 'A'
  
  $ cd ..


Specify only base (from 1 onto 8):

  $ cp -R a2heads a6
  $ cd a6

  $ hg rebase --base 'desc("D")'
  rebasing 112478962961 "B"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  o  9ddd1ab93e66 'D'
  │
  o  5c73b5224523 'C'
  │
  o  0f744df8b10a 'B'
  │
  @  3e65a434aea7 'I'
  │
  o  4ea5b230dea3 'H'
  │
  o  8908a377a434 'F'
  │
  o  426bada5c675 'A'
  
  $ cd ..


Specify source and dest (from 2 onto 7):

  $ cp -R a2heads a7
  $ cd a7

  $ hg rebase --source 'desc(C)' --dest 'desc(H)'
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  o  21a2359f8c06 'D'
  │
  o  df521e8f6ec4 'C'
  │
  │ o  112478962961 'B'
  │ │
  │ │ @  3e65a434aea7 'I'
  ├───╯
  o │  4ea5b230dea3 'H'
  │ │
  o │  8908a377a434 'F'
  ├─╯
  o  426bada5c675 'A'
  
  $ cd ..


Specify base and dest (from 1 onto 7):

  $ cp -R a a8
  $ cd a8

  $ hg rebase --base 'desc(D)' --dest 'desc(H)'
  rebasing 112478962961 "B"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  o  49ddef00e564 'D'
  │
  o  8a25ca8f8a1c 'C'
  │
  o  59672d756804 'B'
  │
  │ o    c6001eacfde5 'G'
  │ ├─╮
  │ │ o  7fb047a69f22 'E'
  │ │ │
  │ │ │ o  3e65a434aea7 'I' master
  ├─────╯
  o │ │  4ea5b230dea3 'H'
  ├─╯ │
  o   │  8908a377a434 'F'
  ├───╯
  o  426bada5c675 'A'
  
  $ cd ..


Specify only revs (from 2 onto 8)

  $ cp -R a2heads a9
  $ cd a9

  $ hg rebase --rev 'desc("C")::'
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"

  $ tglog
  o  53b9858adc18 'D'
  │
  o  b2b0670adeee 'C'
  │
  │ o  112478962961 'B'
  │ │
  @ │  3e65a434aea7 'I'
  │ │
  o │  4ea5b230dea3 'H'
  │ │
  o │  8908a377a434 'F'
  ├─╯
  o  426bada5c675 'A'
  
  $ cd ..

Rebasing both a single revision and a merge in one command

  $ cp -R a aX
  $ cd aX
  $ hg rebase -r $D -r $G --dest 'desc(I)'
  rebasing c6001eacfde5 "G"
  rebasing f585351a92f8 "D"
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
