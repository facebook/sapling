
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
  $ cd ..

convert with datesort

  $ hg convert --datesort t t-datesort
  initializing destination t-datesort repository
  scanning source...
  sorting...
  converting...
  8 a0
  7 a1
  6 a2
  5 a3
  4 a4
  3 b0
  2 a5
  1 a6
  0 b1

graph converted repo

  $ hg -R t-datesort glog --template '{rev} "{desc}"\n'
  o  8 "b1"
  |
  | o  7 "a6"
  | |
  | o  6 "a5"
  | |
  o |  5 "b0"
  | |
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
  8 a0
  7 a1
  6 a2
  5 a3
  4 b0
  3 a4
  2 a5
  1 a6
  0 b1

graph converted repo

  $ hg -R t-sourcesort glog --template '{rev} "{desc}"\n'
  o  8 "b1"
  |
  | o  7 "a6"
  | |
  | o  6 "a5"
  | |
  | o  5 "a4"
  | |
  o |  4 "b0"
  | |
  | o  3 "a3"
  | |
  | o  2 "a2"
  | |
  | o  1 "a1"
  |/
  o  0 "a0"
  
