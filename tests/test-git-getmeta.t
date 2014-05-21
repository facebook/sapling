  $ echo "[extensions]" >> $HGRCPATH
  $ echo "gitlookup = $TESTDIR/../gitlookup.py" >> $HGRCPATH

Set up the hg-git files
  $ hg init repo1
  $ cd repo1/.hg
  $ echo '0000000000000000000000000000000000000000 ffffffffffffffffffffffffffffffffffffffff' > git-mapfile
  $ echo '0000000000000000000000000000000000000000 default/master' > git-remote-refs
  $ echo '0000000000000000000000000000000000000000 0.1' > git-tags


  $ cd ../..
  $ hg clone repo1 repo2 -q
  $ cd repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (183 bytes)

  $ cat .hg/git-mapfile
  0000000000000000000000000000000000000000 ffffffffffffffffffffffffffffffffffffffff
  $ cat .hg/git-remote-refs
  0000000000000000000000000000000000000000 default/master
  $ cat .hg/git-tags
  0000000000000000000000000000000000000000 0.1

Change a file upstream and see that it gets reflected her
  $ echo '1111111111111111111111111111111111111111 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee' >> ../repo1/.hg/git-mapfile
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (265 bytes)

  $ cat .hg/git-mapfile
  0000000000000000000000000000000000000000 ffffffffffffffffffffffffffffffffffffffff
  1111111111111111111111111111111111111111 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
