  $ echo "[extensions]" >> $HGRCPATH
  $ echo "gitlookup = $TESTDIR/../gitlookup.py" >> $HGRCPATH
  $ echo "gitrevset = $TESTDIR/../gitrevset.py" >> $HGRCPATH
  $ echo '[ui]' >> $HGRCPATH
  $ echo 'ssh = python "$RUNTESTDIR/dummyssh"' >> $HGRCPATH

Set up the hg-git files
  $ hg init repo1
  $ cd repo1
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log -r . --template '{node}\n'
  3903775176ed42b1458a6281db4a0ccf4d9f287a
  $ cd .hg
  $ echo "ffffffffffffffffffffffffffffffffffffffff 3903775176ed42b1458a6281db4a0ccf4d9f287a" > git-mapfile
  $ echo 'ffffffffffffffffffffffffffffffffffffffff default/master' > git-remote-refs
  $ echo 'ffffffffffffffffffffffffffffffffffffffff 0.1' > git-tags
  $ echo '[gitlookup]' >> hgrc
  $ echo "mapfile = $TESTTMP/repo1/.hg/git-mapfile" >> hgrc

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
  ffffffffffffffffffffffffffffffffffffffff 3903775176ed42b1458a6281db4a0ccf4d9f287a
  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master
  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

  $ echo '1111111111111111111111111111111111111111 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee' >> ../repo1/.hg/git-mapfile
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (265 bytes)

  $ cat .hg/git-mapfile
  ffffffffffffffffffffffffffffffffffffffff 3903775176ed42b1458a6281db4a0ccf4d9f287a
  1111111111111111111111111111111111111111 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee

  $ cd ..
  $ hg clone ssh://user@dummy/repo1 repo3 -q
  $ cd repo3
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (265 bytes)

  $ cat .hg/git-mapfile
  ffffffffffffffffffffffffffffffffffffffff 3903775176ed42b1458a6281db4a0ccf4d9f287a
  1111111111111111111111111111111111111111 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master
  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Change a file upstream and see that it gets reflected here
  $ echo '2222222222222222222222222222222222222222 dddddddddddddddddddddddddddddddddddddddd' >> ../repo1/.hg/git-mapfile
  $ echo 'releases/foo1 foo1' >> ../repo1/.hg/git-named-branches
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (366 bytes)

  $ cat .hg/git-mapfile
  ffffffffffffffffffffffffffffffffffffffff 3903775176ed42b1458a6281db4a0ccf4d9f287a
  1111111111111111111111111111111111111111 eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
  2222222222222222222222222222222222222222 dddddddddddddddddddddddddddddddddddddddd

  $ cd ..
  $ hg clone ssh://user@dummy/repo1 repo-ssh -q
  $ cd repo-ssh

Check that our revset and template mappings work
  $ hg log -r "gitnode(ffffffffffffffffffffffffffffffffffffffff)" --template "{node}\n"
  3903775176ed42b1458a6281db4a0ccf4d9f287a

  $ hg log -r 'gffffffffffffffffffffffffffffffffffffffff' --template "{node}\n"
  3903775176ed42b1458a6281db4a0ccf4d9f287a

  $ hg log -r . --template "{gitnode}\n"
  ffffffffffffffffffffffffffffffffffffffff

  $ touch b
  $ hg add b
  $ hg ci -mb
  $ hg log -r . --template "{gitnode}\n"
  
