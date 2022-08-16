#chg-compatible
#debugruntest-compatible

  $ . "$TESTDIR/library.sh"

Do some initial setup
  $ CACHEDIR="$TESTTMP/hgcache"
  $ configure dummyssh
  $ enable treemanifest remotefilelog pushrebase remotenames
  $ setconfig remotefilelog.reponame=testrepo remotefilelog.cachepath="$CACHEDIR"
  $ setconfig treemanifest.sendtrees=true
  $ setconfig ui.username="nobody <no.reply@fb.com>"

Setup a server repo
  $ hginit server
  $ cd server
  $ cat >> .hg/hgrc <<CONFIG
  > [treemanifest]
  > server = True
  > [remotefilelog]
  > server = True
  > shallowtrees = True
  > CONFIG
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

  $ hg bookmark master_bookmark -r tip
  $ hg log -r tip -q
  26805aba1e60

Send unbundle
  $ cat $TESTDIR/bundles/sendunbundle.test.hg | hg debugsendunbundle ssh://user@dummy/server
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded

Server tip is now different
  $ cd "$TESTTMP/server"
  $ hg log -r tip -q
  c2e526aacb51
