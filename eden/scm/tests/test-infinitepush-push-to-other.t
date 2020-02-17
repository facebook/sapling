#chg-compatible

  $ disable treemanifest

Setup the test
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ enable infinitepush pushrebase
  $ cp "$HGRCPATH" "$TESTTMP/defaulthgrc"
  $ hg init repo1
  $ cd repo1
  $ setupserver
  $ cd ..
  $ hg init repo2
  $ cd repo2
  $ setupserver
  $ cd ..
  $ hg init repo3
  $ cd repo3
  $ setupserver
  $ cd ..

Check that we replicate a push
  $ hg clone ssh://user@dummy/repo1 client -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > default-push=ssh://user@dummy/repo3
  > infinitepush=ssh://user@dummy/repo1
  > infinitepush-other=ssh://user@dummy/repo2
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client
  $ mkcommit initialcommit
  $ hg push -r . --to scratch/test123 --create
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  please wait while we replicate this push to an alternate repository
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ mkcommit morecommit
  $ hg push infinitepush -r . --to scratch/test123
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  please wait while we replicate this push to an alternate repository
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  $ mkcommit anothercommit
  $ hg push default -r . --to scratch/test123
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 3 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  remote:     17528c345014  anothercommit
  please wait while we replicate this push to an alternate repository
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 3 commits:
  remote:     67145f466344  initialcommit
  remote:     6b2f28e02245  morecommit
  remote:     17528c345014  anothercommit
  $ cd ..

Check that we do not replicate a push to the same destination
  $ hg clone ssh://user@dummy/repo1 client2 -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > infinitepush-other=ssh://user@dummy/repo1
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client2
  $ mkcommit initialcommit
  $ hg push -r . --to scratch/test456 --create
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ cd ..
Check that we do not replicate a push when the destination is set
  $ hg clone ssh://user@dummy/repo1 client3 -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > infinitepush-other=ssh://user@dummy/repo2
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client3
  $ mkcommit initialcommit
  $ hg push ssh://user@dummy/repo3 -r . --to scratch/test789 --create
  pushing to ssh://user@dummy/repo3
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ cd ..
