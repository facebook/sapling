  $ . helpers-usechg.sh

  $ setconfig ui.allowemptycommit=1
  $ enable convert

  $ hg init t
  $ cd t
  $ echo a >> a
  $ hg ci -qAm a0 -d '1 0'
  $ echo a >> a
  $ hg ci -m a1 -d '2 0'
  $ echo a >> a
  $ hg ci -m a2 -d '3 0'
  $ echo a >> a
  $ hg ci -m a3 -d '4 0'
  $ hg book -i brancha

  $ hg up -Cq 0
  $ echo b >> b
  $ hg ci -qAm b0 -d '6 0'
  $ hg book -i branchb

  $ hg up -qC brancha
  $ echo a >> a
  $ hg ci -m a4 -d '5 0'
  $ echo a >> a
  $ hg ci -m a5 -d '7 0'
  $ echo a >> a
  $ hg ci -m a6 -d '8 0'

  $ hg up -qC branchb
  $ echo b >> b
  $ hg ci -m b1 -d '9 0'

  $ hg up -qC 0
  $ echo c >> c
  $ hg ci -qAm c0 -d '10 0'
  $ hg bookmark branchc

  $ hg up -qC brancha
  $ hg ci -qm a7x -d '11 0'

  $ hg up -qC branchb
  $ hg ci -m b2x -d '12 0'

  $ hg up -qC branchc
  $ hg merge branchb -q

  $ hg ci -m c1 -d '13 0'
  $ hg bookmark -d brancha branchb branchc
  $ cd $TESTTMP

convert with datesort

  $ hg convert --datesort t t-datesort
  initializing destination t-datesort repository
  scanning source...
  sorting...
  converting...
  12 a0
  11 a1
  10 a2
  9 a3
  8 a4
  7 b0
  6 a5
  5 a6
  4 b1
  3 c0
  2 a7x
  1 b2x
  0 c1

graph converted repo

  $ hg -R t-datesort log -G --template '{rev} "{desc}"\n'
  o    12 "c1"
  |\
  | o  11 "b2x"
  | |
  | | o  10 "a7x"
  | | |
  o | |  9 "c0"
  | | |
  | o |  8 "b1"
  | | |
  | | o  7 "a6"
  | | |
  | | o  6 "a5"
  | | |
  | o |  5 "b0"
  |/ /
  | o  4 "a4"
  | |
  | o  3 "a3"
  | |
  | o  2 "a2"
  | |
  | o  1 "a1"
  |/
  o  0 "a0"
  

convert with datesort (default mode)

  $ hg convert t t-sourcesort
  initializing destination t-sourcesort repository
  scanning source...
  sorting...
  converting...
  12 a0
  11 a1
  10 a2
  9 a3
  8 b0
  7 a4
  6 a5
  5 a6
  4 b1
  3 c0
  2 a7x
  1 b2x
  0 c1

graph converted repo

  $ hg -R t-sourcesort log -G --template '{rev} "{desc}"\n'
  o    12 "c1"
  |\
  | o  11 "b2x"
  | |
  | | o  10 "a7x"
  | | |
  o | |  9 "c0"
  | | |
  | o |  8 "b1"
  | | |
  | | o  7 "a6"
  | | |
  | | o  6 "a5"
  | | |
  | | o  5 "a4"
  | | |
  | o |  4 "b0"
  |/ /
  | o  3 "a3"
  | |
  | o  2 "a2"
  | |
  | o  1 "a1"
  |/
  o  0 "a0"
  

convert with closesort

  $ hg convert --closesort t t-closesort
  initializing destination t-closesort repository
  scanning source...
  sorting...
  converting...
  12 a0
  11 a1
  10 a2
  9 a3
  8 b0
  7 a4
  6 a5
  5 a6
  4 b1
  3 c0
  2 a7x
  1 b2x
  0 c1

graph converted repo

  $ hg -R t-closesort log -G --template '{rev} "{desc}"\n'
  o    12 "c1"
  |\
  | o  11 "b2x"
  | |
  | | o  10 "a7x"
  | | |
  o | |  9 "c0"
  | | |
  | o |  8 "b1"
  | | |
  | | o  7 "a6"
  | | |
  | | o  6 "a5"
  | | |
  | | o  5 "a4"
  | | |
  | o |  4 "b0"
  |/ /
  | o  3 "a3"
  | |
  | o  2 "a2"
  | |
  | o  1 "a1"
  |/
  o  0 "a0"
  
