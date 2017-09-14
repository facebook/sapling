  $ echo "[extensions]" >> $HGRCPATH
  $ echo "gitlookup = $TESTDIR/../hgext3rd/gitlookup.py" >> $HGRCPATH
  $ echo "gitrevset = $TESTDIR/../hgext3rd/gitrevset.py" >> $HGRCPATH
  $ echo "strip = " >> $HGRCPATH
  $ echo "[ui]" >> $HGRCPATH
  $ echo 'ssh = python "$RUNTESTDIR/dummyssh"' >> $HGRCPATH

Set up the hg-git files
-----------------------

  $ hg init repo1
  $ cd repo1
  $ touch a
  $ hg add a
  $ hg ci -m "adding a"
  $ hg log -r . --template "{node}\n"
  fc5f87aa174b7d4016abf3e908fd63cc99774540

Add the corresponding git metadata for the commit
  $ cd .hg
  $ echo "ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540" > git-mapfile
  $ echo "ffffffffffffffffffffffffffffffffffffffff default/master" > git-remote-refs
  $ echo "ffffffffffffffffffffffffffffffffffffffff 0.1" > git-tags
  $ echo "[gitlookup]" >> hgrc
  $ echo "mapfile = $TESTTMP/repo1/.hg/git-mapfile" >> hgrc

Clone new repo from local repo and check that git metadata syncs properly
-------------------------------------------------------------------------

  $ cd ../..
  $ hg clone repo1 repo2 -q
  $ cd repo2

  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (183 bytes)

  $ sort .hg/git-mapfile
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Clone new repo from remote repo and check that git metadata syncs properly
--------------------------------------------------------------------------

  $ cd ..
  $ hg clone ssh://user@dummy/repo1 repo3 -q
  $ cd repo3
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (183 bytes)

  $ sort .hg/git-mapfile
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Make changes upstream and check that they get reflected in clones
-----------------------------------------------------------------

  $ cd ../repo1
  $ echo >> a
  $ hg ci -m "modifying a"
  $ hg log -r . --template "{node}\n"
  d4a59f7c570a8794e6ec20865090e7b848395b92
  $ echo "1111111111111111111111111111111111111111 d4a59f7c570a8794e6ec20865090e7b848395b92" >> .hg/git-mapfile

Check local repo syncing
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (265 bytes)

  $ sort .hg/git-mapfile
  1111111111111111111111111111111111111111 d4a59f7c570a8794e6ec20865090e7b848395b92
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Make more changes upstream
  $ cd ../repo1
  $ echo >> a
  $ hg ci -m "modifying a"
  $ hg log -r . --template "{node}\n"
  c411819f7fd6036d50b17a28d3edb7aa9121985a
  $ echo "2222222222222222222222222222222222222222 c411819f7fd6036d50b17a28d3edb7aa9121985a" >> .hg/git-mapfile
  $ hg update -q -r "tip^"
  $ echo >> a
  $ hg ci -qm "creating new head with modified a"
  $ hg log -r . --template "{node}\n"
  8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91
  $ echo "3333333333333333333333333333333333333333 8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91" >> .hg/git-mapfile
  $ echo "releases/foo1 foo1" >> .hg/git-named-branches

Check remote repo syncing
  $ cd ../repo3
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (448 bytes)

  $ sort .hg/git-mapfile
  1111111111111111111111111111111111111111 d4a59f7c570a8794e6ec20865090e7b848395b92
  2222222222222222222222222222222222222222 c411819f7fd6036d50b17a28d3edb7aa9121985a
  3333333333333333333333333333333333333333 8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-named-branches
  releases/foo1 foo1

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Strip changes upstream and see that they get reflected in clones
----------------------------------------------------------------

  $ cd ../repo1
  $ hg strip . -q
  $ grep -v "8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91" .hg/git-mapfile > tempfile
  $ mv tempfile .hg/git-mapfile

Check local repo syncing
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (366 bytes)

  $ sort .hg/git-mapfile
  1111111111111111111111111111111111111111 d4a59f7c570a8794e6ec20865090e7b848395b92
  2222222222222222222222222222222222222222 c411819f7fd6036d50b17a28d3edb7aa9121985a
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-named-branches
  releases/foo1 foo1

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Strip some more changes upstream
  $ cd ../repo1
  $ hg strip . -q
  $ grep -v "c411819f7fd6036d50b17a28d3edb7aa9121985a\|d4a59f7c570a8794e6ec20865090e7b848395b92" .hg/git-mapfile > tempfile
  $ mv tempfile .hg/git-mapfile

Add a new head upstream
  $ echo a >> a
  $ hg ci -qm "modifying a"
  $ hg log -r . --template "{node}\n"
  3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
  $ echo "2222222222222222222222222222222222222222 3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6" >> .hg/git-mapfile

Check remote repo syncing
  $ cd ../repo3
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (284 bytes)

  $ sort .hg/git-mapfile
  2222222222222222222222222222222222222222 3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-named-branches
  releases/foo1 foo1

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Check local repo syncing
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (284 bytes)

  $ sort .hg/git-mapfile
  2222222222222222222222222222222222222222 3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-named-branches
  releases/foo1 foo1

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Create unrelated history upstream and check that the syncing works
------------------------------------------------------------------

  $ cd ../repo1
  $ hg update -q null
  $ touch b
  $ hg ci -Aqm "adding b"
  $ hg log -r . --template "{node}\n"
  627ddeb6657d60a21b87c725b5c4e60d91b75f19
  $ echo "3333333333333333333333333333333333333333 627ddeb6657d60a21b87c725b5c4e60d91b75f19" >> .hg/git-mapfile

Check local repo syncing
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (366 bytes)

  $ sort .hg/git-mapfile
  2222222222222222222222222222222222222222 3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
  3333333333333333333333333333333333333333 627ddeb6657d60a21b87c725b5c4e60d91b75f19
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-named-branches
  releases/foo1 foo1

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Check that our revset and template mappings work
------------------------------------------------

Clone a new repo
  $ cd ..
  $ hg clone ssh://user@dummy/repo1 repo-ssh -q
  $ cd repo-ssh

  $ hg log -r "gitnode(ffffffffffffffffffffffffffffffffffffffff)" --template "{node}\n"
  fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ hg log -r "gffffffffffffffffffffffffffffffffffffffff" --template "{node}\n"
  fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ hg log -r . --template "{gitnode}\n"
  3333333333333333333333333333333333333333

  $ touch c
  $ hg add c
  $ hg ci -mc
  $ hg log -r . --template "{gitnode}\n"
  

Check that gitnode revset and template work on the server
  $ cd ../repo1
  $ hg log -r . --template "{node}-{gitnode}\n"
  627ddeb6657d60a21b87c725b5c4e60d91b75f19-3333333333333333333333333333333333333333
  $ hg log -r "gitnode(ffffffffffffffffffffffffffffffffffffffff)" --template "{node}\n"
  fc5f87aa174b7d4016abf3e908fd63cc99774540
  $ hg log -r "gitnode(unknown)" --template "{node}\n"
  abort: unknown revision 'unknown'!
  [255]

Check that using revision numbers instead of hashes still works. Use `bundle` command
because it calls `repo.lookup(...)` with int argument
  $ touch c
  $ hg add c
  $ hg ci -m "adding c"
  $ hg bundle -r . --base 0 file.txt
  2 changesets found
