#debugruntest-compatible

  $ enable rebase
  $ setconfig rebase.experimental.inmemory=True
  $ configure modernclient

Prepare repo

  $ newclientrepo repo1
  $ drawdag <<'EOS'
  > C   # C/x = 1\n3\n 
  > |
  > | B # B/x = 1\n2\n
  > |/
  > A   # A/x = 1\n
  > EOS

# rebase union should succeed

  $ hg rebase -r $C -d $B -t :union
  rebasing 2c13bd228f8e "C"
  merging x
