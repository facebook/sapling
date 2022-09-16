#require fsmonitor

  $ configure modernclient
  $ setconfig status.use-rust=False
  $ . "$TESTDIR/histedit-helpers.sh"
  $ enable histedit fsmonitor rebase hgevents sparse
  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newclientrepo repo
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
  o  commit:      f585351a92f8
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     D
  │
  o  commit:      26805aba1e60
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     C
  │
  o  commit:      112478962961
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     B
  │
  o  commit:      426bada5c675
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
  @  commit:      ded77c342953
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     D
  │
  o  commit:      508221a61cea
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     B
  │
  o  commit:      088d21ab9b28
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     C
  │
  o  commit:      426bada5c675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
