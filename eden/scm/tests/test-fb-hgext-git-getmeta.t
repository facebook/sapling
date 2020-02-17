#chg-compatible

#chg-compatible

  $ disable treemanifest
#testcases onlymapdelta.true onlymapdelta.false

  $ configure dummyssh
  $ enable gitlookup gitrevset

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

Simulate config change from serving complete hg map to only missing delta
-------------------------------------------------------------------------

Making this change here instead of during repo setup earlier ensures that we
test scenarios where the config changes after repos have been syncing.
#if onlymapdelta.true
  $ cd ../repo1/.hg
  $ echo "onlymapdelta = True" >> hgrc
  $ cd ..
#endif

Clone new repo from remote repo and check that git metadata syncs properly
--------------------------------------------------------------------------

  $ cd ..
  $ hg clone ssh://user@dummy/repo1 repo3 -q
  $ cd repo3
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (223 bytes)

  $ sort .hg/git-synced-hgheads
  fc5f87aa174b7d4016abf3e908fd63cc99774540
#else
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (183 bytes)
#endif

  $ sort .hg/git-mapfile
  ffffffffffffffffffffffffffffffffffffffff fc5f87aa174b7d4016abf3e908fd63cc99774540

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1

Redundant sync just to see that the hg-git map file is not synced with
onlymapdelta being True
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 2 files (101 bytes)

  $ cat .hg/git-remote-refs
  ffffffffffffffffffffffffffffffffffffffff default/master

  $ cat .hg/git-tags
  ffffffffffffffffffffffffffffffffffffffff 0.1
#endif

Make changes upstream and check that they get reflected in clones
-----------------------------------------------------------------

  $ cd ../repo1
  $ echo >> a
  $ hg ci -m "modifying a"
  $ hg log -r . --template "{node}\n"
  d4a59f7c570a8794e6ec20865090e7b848395b92
  $ echo "1111111111111111111111111111111111111111 d4a59f7c570a8794e6ec20865090e7b848395b92" >> .hg/git-mapfile
  $ sort -k 2 -o .hg/git-mapfile.bak .hg/git-mapfile
  $ mv .hg/git-mapfile.bak .hg/git-mapfile

Check local repo syncing
  $ cd ../repo2
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (305 bytes)

  $ sort .hg/git-synced-hgheads
  d4a59f7c570a8794e6ec20865090e7b848395b92
#else
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (265 bytes)
#endif

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
  $ sort -k 2 -o .hg/git-mapfile.bak .hg/git-mapfile
  $ mv .hg/git-mapfile.bak .hg/git-mapfile
  $ hg update -q -r "tip^"
  $ echo >> a
  $ hg ci -qm "creating new head with modified a"
  $ hg log -r . --template "{node}\n"
  8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91
  $ echo "3333333333333333333333333333333333333333 8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91" >> .hg/git-mapfile
  $ sort -k 2 -o .hg/git-mapfile.bak .hg/git-mapfile
  $ mv .hg/git-mapfile.bak .hg/git-mapfile
  $ echo "releases/foo1 foo1" >> .hg/git-named-branches

Check remote repo syncing
  $ cd ../repo3
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (529 bytes)

  $ sort .hg/git-synced-hgheads
  8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91
  c411819f7fd6036d50b17a28d3edb7aa9121985a
#else
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (448 bytes)
#endif

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
  $ hg debugstrip . -q
  $ grep -v "8ea31c3efb6d2edb6d9fe608c29034e7e7ed5f91" .hg/git-mapfile > tempfile
  $ mv tempfile .hg/git-mapfile

Check local repo syncing
  $ cd ../repo2
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (406 bytes)

  $ sort .hg/git-synced-hgheads
  c411819f7fd6036d50b17a28d3edb7aa9121985a
#else
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (366 bytes)
#endif

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
  $ hg debugstrip . -q
  $ grep -v "c411819f7fd6036d50b17a28d3edb7aa9121985a\|d4a59f7c570a8794e6ec20865090e7b848395b92" .hg/git-mapfile > tempfile
  $ mv tempfile .hg/git-mapfile

Add a new head upstream
  $ echo a >> a
  $ hg ci -qm "modifying a"
  $ hg log -r . --template "{node}\n"
  3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
  $ echo "2222222222222222222222222222222222222222 3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6" >> .hg/git-mapfile
  $ sort -k 2 -o .hg/git-mapfile.bak .hg/git-mapfile
  $ mv .hg/git-mapfile.bak .hg/git-mapfile

Check remote repo syncing
  $ cd ../repo3
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (324 bytes)

  $ sort .hg/git-synced-hgheads
  3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
#else
  $ hg gitgetmeta -v
  getting git metadata from ssh://user@dummy/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (284 bytes)
#endif

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
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (324 bytes)

  $ sort .hg/git-synced-hgheads
  3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
#else
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (284 bytes)
#endif

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
  $ sort -k 2 -o .hg/git-mapfile.bak .hg/git-mapfile
  $ mv .hg/git-mapfile.bak .hg/git-mapfile

Check local repo syncing
  $ cd ../repo2
#if onlymapdelta.true
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (447 bytes)

  $ sort .hg/git-synced-hgheads
  3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6
  627ddeb6657d60a21b87c725b5c4e60d91b75f19
#else
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 4 files (366 bytes)
#endif

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

Check corrupted git-hg map data
-------------------------------

This is only valid when we are serving missing map data because when we serve
the complete map, we just simply serve the file without any validations on the
map data.
#if onlymapdelta.true
  $ cd ../repo1
  $ touch a
  $ hg ci -Aqm "adding a"
  $ hg log -r . --template "{node}\n"
  1e2e1480acd77a0155ee53e30aab1bb4a08f9f22

Not updating the map file intentionally to simulate missing map data. Instead,
we try to sync changes and check that the syncing fails.
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  abort: gitmeta: missing hashes in file $TESTTMP/repo1/.hg/git-mapfile
  [255]

With gitlookup.skiphashes, gitgetmeta should work with missing map data.
  $ cd ../repo1
  $ cp .hg/hgrc .hg/hgrc.bak
  $ echo "skiphashes = 1e2e1480acd77a0155ee53e30aab1bb4a08f9f22" >> .hg/hgrc
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 3 files (120 bytes)

With gitlookup.skiphashes, non-skipped, missing map data will still abort.
  $ rm -f .hg/git-*
  $ cd $TESTTMP/repo1
  $ mv .hg/git-mapfile .hg/git-mapfile-bak
  $ grep -v "3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6" .hg/git-mapfile-bak > .hg/git-mapfile
  $ cd $TESTTMP/repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  abort: gitmeta: missing hashes in file $TESTTMP/repo1/.hg/git-mapfile
  [255]

With gitlookup.skiphashes, initial gitgetmeta will work.
  $ echo "skiphashes = 1e2e1480acd77a0155ee53e30aab1bb4a08f9f22 3bfa460515b210d1e6f7e21bde166ef5c5f0d9b6" >> $TESTTMP/repo1/.hg/hgrc
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (365 bytes)

Restore git data in repo1 and repo2, as well as the config for repo1
  $ cd ../repo1
  $ mv .hg/git-mapfile-bak .hg/git-mapfile
  $ mv .hg/hgrc.bak .hg/hgrc
  $ cd ../repo2
  $ echo "627ddeb6657d60a21b87c725b5c4e60d91b75f19" > .hg/git-synced-hgheads

Now adding the map entries to both the repos to simulate corruption on the
client side
  $ cd ../repo1
  $ echo "4444444444444444444444444444444444444444 1e2e1480acd77a0155ee53e30aab1bb4a08f9f22" >> .hg/git-mapfile
  $ sort -k 2 -o .hg/git-mapfile.bak .hg/git-mapfile
  $ mv .hg/git-mapfile.bak .hg/git-mapfile
  $ cp .hg/git-mapfile ../repo2/.hg/git-mapfile
  $ cd ../repo2
  $ hg gitgetmeta -v
  getting git metadata from $TESTTMP/repo1
  warning: gitmeta: unexpected lines in .hg/git-mapfile
  writing .hg/git-mapfile
  writing .hg/git-synced-hgheads
  writing .hg/git-named-branches
  writing .hg/git-remote-refs
  writing .hg/git-tags
  wrote 5 files (529 bytes)

Strip the last commit and restore map entries to have same state as
onlymapdelta.false

  $ grep -v "1e2e1480acd77a0155ee53e30aab1bb4a08f9f22" .hg/git-mapfile > tempfile
  $ mv tempfile .hg/git-mapfile
  $ cd ../repo1
  $ hg debugstrip -q "tip"
  $ grep -v "1e2e1480acd77a0155ee53e30aab1bb4a08f9f22" .hg/git-mapfile > tempfile
  $ mv tempfile .hg/git-mapfile
#endif

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
