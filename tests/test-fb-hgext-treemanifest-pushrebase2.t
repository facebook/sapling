Push merge commits from a treeonly shallow repo to a hybrid treemanifest server

  $ setconfig remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache ui.ssh="python $TESTDIR/dummyssh"

  $ newrepo server
  $ setconfig treemanifest.server=True
  $ enable pushrebase treemanifest

  $ newrepo client
  $ echo remotefilelog >> .hg/requires
  $ enable treemanifest remotefilelog pushrebase
  $ setconfig treemanifest.sendtrees=True treemanifest.treeonly=True
  $ drawdag <<'EOS'
  > D
  > |\
  > B E   # E/F2 = F (renamed from F)
  > | |   # B/A2 = A (renamed from A)
  > A F
  > EOS

  $ hg push --to foo -r $D -f  ssh://user@dummy/server
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 5 changesets:
  remote:     426bada5c675  A
  remote:     a6661b868de9  F
  remote:     9f93d39c36cf  B
  remote:     fc0baf5da824  E
  remote:     5a587c09248a  D

Verify the renames are preserved (commit hashes did not change)

  $ cd $TESTTMP/server
  $ hg log -r "::$D" -G -T "{desc}"
  o    D
  |\
  | o  E
  | |
  o |  B
  | |
  | o  F
  |
  o  A
  

