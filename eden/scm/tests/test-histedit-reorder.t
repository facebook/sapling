#require fsmonitor

  $ . "$TESTDIR/histedit-helpers.sh"
  $ enable histedit fsmonitor rebase hgevents sparse
  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newrepo
  $ hg status --debug
  warning: watchman has recently started (pid *) - operation will be slower than usual (glob)
  poststatusfixup decides to wait for wlock since watchman reported fresh instance

  $ drawdag << 'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg log --graph
  o  changeset:   3:f585351a92f8
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     D
  |
  o  changeset:   2:26805aba1e60
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     C
  |
  o  changeset:   1:112478962961
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     B
  |
  o  changeset:   0:426bada5c675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
  $ hg sparse include B C D
  $ hg co $D
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg histedit 112478962961 --commands - 2>&1 << EOF | fixbundle
  > pick 26805aba1e60 C
  > pick 112478962961 B
  > pick f585351a92f8 D
  > EOF

  $ hg log --graph
  @  changeset:   6:ded77c342953
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     D
  |
  o  changeset:   5:508221a61cea
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     B
  |
  o  changeset:   4:088d21ab9b28
  |  parent:      0:426bada5c675
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     C
  |
  o  changeset:   0:426bada5c675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
