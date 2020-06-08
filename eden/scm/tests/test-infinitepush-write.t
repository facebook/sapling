#chg-compatible

  $ enable remotefilelog
  $ enable treemanifest

Setup the test
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ enable infinitepush pushrebase
  $ cat >> "$HGRCPATH" << EOF
  > [treemanifest]
  > sendtrees=True
  > treeonly=True
  > EOF
  $ cp "$HGRCPATH" "$TESTTMP/defaulthgrc"

  $ hg init repo1
  $ cd repo1
  $ setupserver
  $ cat >> .hg/hgrc << EOF
  > [treemanifest]
  > server=true
  > EOF
  $ cd ..

  $ hg init repo2
  $ cd repo2
  $ setupserver
  $ cat >> .hg/hgrc << EOF
  > [treemanifest]
  > server=true
  > EOF
  $ cd ..

  $ hg init repo3
  $ cd repo3
  $ setupserver
  $ cat >> .hg/hgrc << EOF
  > [treemanifest]
  > server=true
  > EOF
  $ cd ..

Check that we push to the write path if it is present
  $ hg clone ssh://user@dummy/repo1 client -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > default-push=ssh://user@dummy/repo3
  > infinitepush=ssh://user@dummy/repo1
  > infinitepush-write=ssh://user@dummy/repo2
  > [remotefilelog]
  > fallbackpath=ssh://user@dummy/repo2
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client
  $ mkcommit initialcommit
  $ hg push -r . --to scratch/test123 --create
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ mkcommit morecommit
  $ hg push infinitepush -r . --to scratch/test123
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  $ mkcommit anothercommit
  $ hg push default -r . --to scratch/test123
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 3 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  remote:     17528c345014  anothercommit

-- check  that we fallback to non-write path, when write path is not there
  $ mkcommit yetanothercommit
  $ hg push -r . --to scratch/test123 --create --config paths.infinitepush-write=
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 4 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  remote:     17528c345014  anothercommit
  remote:     8785135185c9  yetanothercommit
  $ cd ..

Check that we pull/update from the read path, regardless of the write path presence
  $ hg clone ssh://user@dummy/repo1 client2 -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > infinitepush=ssh://user@dummy/repo2
  > infinitepush-write=ssh://user@dummy/repo3
  > [remotefilelog]
  > fallbackpath=ssh://user@dummy/repo2
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client2
  $ mkcommit initialcommit
  $ hg pull -r 67145f466344
  pulling from ssh://user@dummy/repo2
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files
  $ hg update -r 6b2f28e02245
  pulling '6b2f28e02245' from 'ssh://user@dummy/repo2'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

-- check that we can pull from read path, when write path is not present
  $ hg pull -r 8785135185c9 --config paths.infinitepush-write= --config paths.infinitepush=ssh://user@dummy/repo1
  pulling from ssh://user@dummy/repo1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 4 files
  $ cd ..

Check that infinitepush writes can be disabled by a config
  $ cat >> "$TESTTMP/repo1/.hg/hgrc" <<EOF
  > [infinitepush]
  > server.acceptwrites=False
  > EOF
  $ cd "$TESTTMP/client"
  $ mkcommit ababagalamaga
  $ hg push -r . --to scratch/ababagalamaga --create --config paths.infinitepush-write=ssh://user@dummy/repo1
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: infinitepush writes are disabled on this server
  abort: push failed on remote
  [255]


