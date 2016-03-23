This test file aims at test topological iteration and the various configuration it can has.

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate={rev}\n
  > EOF

On this simple example, all topological branch are displayed in turn until we
can finally display 0. this implies skipping from 8 to 3 and coming back to 7
later.

  $ hg init test01
  $ cd test01
  $ hg unbundle $TESTDIR/bundles/remote.hg
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg log -G
  o  8
  |
  | o  7
  | |
  | o  6
  | |
  | o  5
  | |
  | o  4
  | |
  o |  3
  | |
  o |  2
  | |
  o |  1
  |/
  o  0
  

(display all nodes)

  $ hg --config experimental.graph-group-branches=1 log -G
  o  8
  |
  o  3
  |
  o  2
  |
  o  1
  |
  | o  7
  | |
  | o  6
  | |
  | o  5
  | |
  | o  4
  |/
  o  0
  

(revset skipping nodes)

  $ hg --config experimental.graph-group-branches=1 log -G --rev 'not (2+6)'
  o  8
  |
  o  3
  :
  o  1
  |
  | o  7
  | :
  | o  5
  | |
  | o  4
  |/
  o  0
  

(begin) from the other branch

  $ hg --config experimental.graph-group-branches=1 --config experimental.graph-group-branches.firstbranch=5 log -G
  o  7
  |
  o  6
  |
  o  5
  |
  o  4
  |
  | o  8
  | |
  | o  3
  | |
  | o  2
  | |
  | o  1
  |/
  o  0
  
