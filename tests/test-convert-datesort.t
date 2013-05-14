
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert=
  > graphlog=
  > EOF
  $ hg init t
  $ cd t
  $ echo a >> a
  $ hg ci -Am a0 -d '1 0'
  adding a
  $ hg branch brancha
  marked working directory as branch brancha
  (branches are permanent and global, did you want a bookmark?)
  $ echo a >> a
  $ hg ci -m a1 -d '2 0'
  $ echo a >> a
  $ hg ci -m a2 -d '3 0'
  $ echo a >> a
  $ hg ci -m a3 -d '4 0'
  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch branchb
  marked working directory as branch branchb
  (branches are permanent and global, did you want a bookmark?)
  $ echo b >> b
  $ hg ci -Am b0 -d '6 0'
  adding b
  $ hg up -C brancha
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
  $ hg ci -m a4 -d '5 0'
  $ echo a >> a
  $ hg ci -m a5 -d '7 0'
  $ echo a >> a
  $ hg ci -m a6 -d '8 0'
  $ hg up -C branchb
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> b
  $ hg ci -m b1 -d '9 0'
  $ hg up -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c >> c
  $ hg branch branchc
  marked working directory as branch branchc
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Am c0 -d '10 0'
  adding c
  $ hg up -C brancha
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg ci --close-branch -m a7x -d '11 0'
  $ hg up -C branchb
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg ci --close-branch -m b2x -d '12 0'
  $ hg up -C branchc
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge branchb
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m c1 -d '13 0'
  $ cd ..

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

  $ hg -R t-datesort glog --template '{rev} "{desc}"\n'
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

  $ hg -R t-sourcesort glog --template '{rev} "{desc}"\n'
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
  4 a7x
  3 b1
  2 b2x
  1 c0
  0 c1

graph converted repo

  $ hg -R t-closesort glog --template '{rev} "{desc}"\n'
  o    12 "c1"
  |\
  | o  11 "c0"
  | |
  o |  10 "b2x"
  | |
  o |  9 "b1"
  | |
  | | o  8 "a7x"
  | | |
  | | o  7 "a6"
  | | |
  | | o  6 "a5"
  | | |
  | | o  5 "a4"
  | | |
  o | |  4 "b0"
  |/ /
  | o  3 "a3"
  | |
  | o  2 "a2"
  | |
  | o  1 "a1"
  |/
  o  0 "a0"
  
